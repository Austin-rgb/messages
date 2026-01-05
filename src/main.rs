use actix::Actor;
use actix_web::{
    App, HttpResponse, HttpServer, Responder, get, post, rt, web,
    web::{Path, scope},
};
use auth_middleware::{Auth, Claims};
use sqlx::{SqlitePool, query, query_as};
use dotenvy::dotenv;
use std::vec::Vec;
use uuid::Uuid;
mod models;
mod repositories;
mod ws;
mod logging;
use crate::logging::LoggingMiddleware;
use crate::models::{
    AppState, ConversationListItem, ConversationResponse, CreateConversation, CreateMessage,
    MessageFilters, MessageResponse, Participant,
};
use crate::repositories::{
    Conversation, Message, MessageReceipt, Participant as PRepo, is_participant,
};
use crate::ws::{ChatServer, DeliverMessage, ws_route};

async fn init_db(db: &SqlitePool) -> Result<(), sqlx::Error> {
    Message::create_table(db).await;
    Conversation::create_table(db).await;
    MessageReceipt::create_table(db).await;
    PRepo::create_table(db).await;

    // Add indexes
    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_messages_conversation 
        ON messages(conversation, created)
        "#,
    )
    .execute(db)
    .await?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_participants_conversation 
        ON participants(conversation)
        "#,
    )
    .execute(db)
    .await?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_participants_user 
        ON participants(participant)
        "#,
    )
    .execute(db)
    .await?;

    Ok(())
}

#[post("/conversations")]
async fn create_conversation(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    payload: web::Json<CreateConversation>,
) -> impl Responder {
    // Validation
    if payload.participants.is_empty() {
        return HttpResponse::BadRequest().body("Conversation must have at least one participant");
    }

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    let name = payload
        .name
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Check if the name already exists
    let exists: Option<ConversationResponse> = query_as::<_, ConversationResponse>(
        "SELECT name, admin, title, created FROM conversations WHERE name = ?",
    )
    .bind(&name)
    .fetch_optional(&mut *tx)
    .await
    .unwrap(); // handle error properly in real code

    let name = if exists.is_some() {
        Uuid::new_v4().to_string()
    } else {
        name.clone()
    };
    // 1️⃣ Create conversation (admin = creator)

    let conversation =
        match Conversation::insert(&mut *tx, &name, &payload.title, &claims.sub).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Conversation insert error: {:?}", e);
                return HttpResponse::InternalServerError().finish();
            }
        };

    // 2️⃣ Insert creator as participant
    if let Err(e) = PRepo::insert(&mut *tx, &conversation.name, &claims.sub).await {
        eprintln!("Participant insert error: {:?}", e);
        return HttpResponse::InternalServerError().finish();
    }

    // 3️⃣ Insert other participants (deduplicated)
    for user in payload.participants.iter().filter(|u| *u != &claims.sub) {
        if let Err(e) = PRepo::insert(&mut *tx, &conversation.name, user).await {
            eprintln!("Participant insert error: {:?}", e);
            return HttpResponse::InternalServerError().finish();
        }
    }

    // 4️⃣ Commit transaction
    if tx.commit().await.is_err() {
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Ok().json(conversation)
}

#[get("/conversations")]
async fn list_conversations(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
) -> impl Responder {
    let result = query_as::<_, ConversationListItem>(
        r#"
        SELECT
            c.name,
            c.title,
            c.admin,
            c.created
        FROM conversations c
        JOIN participants p
            ON p.conversation = c.name
        WHERE p.participant = ?
        ORDER BY c.created DESC
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.db)
    .await;

    match result {
        Ok(conversations) => HttpResponse::Ok().json(conversations),
        Err(e) => {
            eprintln!("DB error: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[get("/conversations/{name}")]
async fn get_conversation(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    path: Path<String>,
) -> impl Responder {
    let conversation_name = path.into_inner();

    // Check if user is a participant

    if !is_participant(&state.db, &conversation_name, &claims.sub).await {
        return HttpResponse::Forbidden().body("Not a participant in this conversation");
    }

    let conversation = query_as::<_, ConversationResponse>(
        r#"
        SELECT name, title, admin, created
        FROM conversations
        WHERE name = ?
        "#,
    )
    .bind(&conversation_name)
    .fetch_one(&state.db)
    .await;

    match conversation {
        Ok(conv) => HttpResponse::Ok().json(conv),
        Err(sqlx::Error::RowNotFound) => HttpResponse::NotFound().body("Conversation not found"),
        Err(e) => {
            eprintln!("Error fetching conversation: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[post("/conversations/{name}/messages")]
async fn post_message(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    path: Path<String>,
    payload: web::Json<CreateMessage>,
) -> impl Responder {
    let conversation_name = path.into_inner();

    // validate that participation and conversation exists
    let participants: Vec<Participant> =
        repositories::Participant::retrieve(&state.db, &conversation_name, 1000, 0)
            .await
            .expect("failed to fetch participants");
    if !participants.iter().any(|p| p.participant == claims.sub) {
        return HttpResponse::Forbidden().body("Not a participant in this conversation");
    }

    // Insert message
    let result = repositories::Message::insert(
        &state.db,
        &conversation_name,
        &claims.sub,
        &payload.text,
        &payload.reply_to,
    );

    match result.await {
        Ok(message) => {
            let bus = state.chat_server.clone();
            let msg_clone = message.clone();
            let source = message.source.clone();
            let participant_ids: Vec<String> =
                participants.iter().map(|p| p.participant.clone()).collect();
            actix::spawn(async move {
                let payload = serde_json::to_string(&msg_clone).expect("serialization failed");
                for participant in participant_ids {
                    if participant != source {
                        bus.do_send(DeliverMessage {
                            to: participant,
                            payload: payload.clone(),
                        });
                    }
                }
            });
            HttpResponse::Ok().json(message)
        }
        Err(e) => {
            eprintln!("Insert message error: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[get("/conversations/{name}/messages")]
async fn get_messages(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    path: Path<String>,
    query: web::Query<MessageFilters>,
) -> impl Responder {
    let conversation_name = path.into_inner();
    let query = query.into_inner();
    // Check if user is a participant

    if !is_participant(&state.db, &conversation_name, &claims.sub).await {
        return HttpResponse::Forbidden().body("Not a participant in this conversation");
    }

    let messages: Vec<MessageResponse> =
        match repositories::Message::retrieve(&state.db, &conversation_name, query).await {
            Ok(msgs) => msgs,
            Err(e) => {
                eprintln!("Error fetching messages: {:?}", e);
                return HttpResponse::InternalServerError().finish();
            }
        };

    let _messages = messages.clone();
    rt::spawn(async move {
        for msg in _messages.iter() {
            repositories::MessageReceipt::create(&state.db, msg.id, &claims.sub, true, false, None)
                .await;
        }
    });
    HttpResponse::Ok().json(messages)
}

#[get("/messages/{msg}/receipts")]
async fn get_receipts(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    path: Path<i64>,
) -> impl Responder {
    // confirm message ownership
    let msg = path.into_inner();
    match MessageReceipt::retrieve(&state.db, msg).await {
        Ok(receipts) => HttpResponse::Ok().json(receipts),
        Err(e) => {
            eprintln!("error occured in retrieving receipts: {}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[get("/messages/{msg}/react/{reaction}")]
async fn react(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    path: Path<(i64, i64)>,
) -> impl Responder {
    let (msg, reaction) = path.into_inner();
    MessageReceipt::create(&state.db, msg, &claims.sub, false, false, Some(reaction)).await;
    HttpResponse::Ok()
}

#[get("/messages/{msg}/mark_as_read")]
async fn mark_as_read(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    path: Path<i64>,
) -> impl Responder {
    let msg = path.into_inner();
    MessageReceipt::create(&state.db, msg, &claims.sub, false, true, None).await;
    HttpResponse::Ok()
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let db = SqlitePool::connect("sqlite://messages.db?mode=rwc")
        .await
        .expect("Failed to connect to DB");

    init_db(&db).await.expect("DB init failed");
    let chat_server = ChatServer::new().start();
    let state = web::Data::new(AppState { db, chat_server });

    HttpServer::new(move || {
        App::new().app_data(state.clone()).service(
            scope("")
            .wrap(LoggingMiddleware)
                .wrap(Auth)
                .service(create_conversation)
                .service(get_conversation)
                .service(post_message)
                .service(get_messages)
                .service(react)
                .service(mark_as_read)
                .service(list_conversations)
                .service(ws_route),
        )
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
