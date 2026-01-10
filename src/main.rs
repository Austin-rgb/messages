use ::redis::AsyncCommands;
use actix::{Actor, Addr};
use actix_web::{
    App, HttpResponse, HttpServer, Responder, get, post, rt, web,
    web::{Path, scope},
};
use auth_middleware::{Auth, Claims};
use dotenvy::dotenv;
use once_cell::sync::Lazy;
use redis::{Client, RedisResult, Value, aio::Connection, cmd, from_redis_value};
use serde_json::{from_str, to_string};
use sqlx::{SqlitePool, query, query_as};
use std::{collections::HashMap, vec::Vec};
use tokio::sync::RwLock;
use uuid::Uuid;
mod logging;
mod models;
mod redis_cfg;
mod repositories;
mod workers;
mod ws;
use crate::redis_cfg::{create_redis, ensure_group};
use crate::repositories::{
    Conversation, Message, MessageReceipt, Participant as PRepo, is_participant, time_now,
};
use crate::workers::db_worker;
use crate::ws::{ChatServer, DeliverMessage, ws_route};
use crate::{logging::LoggingMiddleware, repositories::is_sender};
use crate::{
    models::{
        AppState, ConversationListItem, ConversationResponse, CreateConversation, CreateMessage,
        InsertMessage, MessageFilters, Participant, Receipt,
    },
    workers::receipt_worker,
};

fn deliver_message(msg: &InsertMessage, targets: Vec<String>, bus: Addr<ChatServer>) {
    let payload = serde_json::to_string(msg).expect("serialization failed");
    for participant in targets {
        bus.do_send(DeliverMessage {
            id: msg.id.clone(),
            to: participant.clone(),
            payload: payload.clone(),
        });
    }
}

static PARTICIPANTS_CACHE: Lazy<RwLock<HashMap<String, Vec<Participant>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

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
    let participants: Vec<String> = payload
        .participants
        .iter()
        .filter(|u| *u != &claims.sub)
        .cloned()
        .collect();

    // if let Err(e) = PRepo::insert_many(&mut *tx, &conversation.name, participants).await {
    //     eprintln!("Participant insert error: {:?}", e);
    //     return HttpResponse::InternalServerError().finish();
    // }
    for participant in participants {
        match PRepo::insert(&mut tx, &conversation.name, &participant).await {
            Ok(_) => (),
            Err(_) => {
                tx.rollback().await.expect("rollback failed");
                return HttpResponse::InternalServerError().finish();
            }
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
    let p = PARTICIPANTS_CACHE
        .read()
        .await
        .get(&conversation_name)
        .cloned();
    let participants: Vec<Participant> = match p {
        None => match repositories::Participant::retrieve(&state.db, &conversation_name, 1000, 0)
            .await
        {
            Ok(p) => {
                PARTICIPANTS_CACHE
                    .write()
                    .await
                    .insert(conversation_name.clone(), p.clone());
                p
            }
            Err(e) => {
                eprintln!("Error getting participants: {:?}", e);
                return HttpResponse::InternalServerError().finish();
            }
        },

        Some(p) => p,
    };

    if !participants.iter().any(|p| p.participant == claims.sub) {
        return HttpResponse::Forbidden().body("Not a participant in this conversation");
    }

    // Insert message
    let bus = state.chat_server.clone();

    let msg = InsertMessage {
        source: claims.sub.clone(),
        conversation: conversation_name,
        text: payload.text.clone(),
        reply_to: payload.reply_to,
        created: time_now(),
        id: Uuid::new_v4().to_string(),
    };
    //let _ = state.writer.send(msg.clone()).await;
    let mut conn = state.redis.get_async_connection().await.unwrap();
    let mss = to_string(&msg).unwrap();
    let res: RedisResult<String> = conn.xadd("messages_stream", "*", &[("payload", mss)]).await;
    if res.is_err() {
        return HttpResponse::ServiceUnavailable().finish();
    }
    let source = msg.source.clone();
    let participant_ids: Vec<String> = participants
        .iter()
        .map(|p| p.participant.clone())
        .filter(|p| *p != *source)
        .collect();
    rt::spawn(async move {
        deliver_message(&msg, participant_ids, bus);
    });

    HttpResponse::Ok().finish()
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

    let messages: Vec<InsertMessage> =
        match repositories::Message::retrieve(&state.db, &conversation_name, query).await {
            Ok(msgs) => msgs,
            Err(e) => {
                eprintln!("Error fetching messages: {:?}", e);
                return HttpResponse::InternalServerError().finish();
            }
        };

    let _messages = messages.clone();
    let events: Vec<Receipt> = messages
        .iter()
        .map(|msg| Receipt {
            message_id: msg.id.clone(),
            user_id: claims.sub.clone(),
            delivered: true,
            read: false,
            reaction: None,
            ts: chrono::Utc::now().timestamp(),
        })
        .collect();
    let redis = state.redis.clone();
    rt::spawn(async move {
        let mut conn = match redis.get_async_connection().await {
            Ok(c) => c,
            Err(_) => return,
        };
        for event in &events {
            let payload = serde_json::to_string(&event).unwrap();

            let _: redis::RedisResult<String> = conn
                .xadd("receipts_stream", "*", &[("payload", payload)])
                .await;
        }
    });
    HttpResponse::Ok().json(messages)
}

#[get("/messages/{msg}/receipts")]
async fn get_receipts(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    path: Path<String>,
) -> impl Responder {
    println!("fetching receipts");
    // confirm message ownership
    let msg = path.into_inner();
    if !is_sender(&state.db, &msg, &claims.sub).await {
        return HttpResponse::NotFound().finish();
    }
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
    path: Path<(String, i64)>,
) -> impl Responder {
    let (msg, reaction) = path.into_inner();
    let event = Receipt {
        message_id: msg,
        user_id: claims.sub.clone(),
        delivered: false,
        read: false,
        reaction: Some(reaction),
        ts: chrono::Utc::now().timestamp(),
    };
    let redis = state.redis.clone();
    rt::spawn(async move {
        let mut conn = match redis.get_async_connection().await {
            Ok(c) => c,
            Err(_) => return,
        };

        let payload = serde_json::to_string(&event).unwrap();

        let _: redis::RedisResult<String> = conn
            .xadd("receipts_stream", "*", &[("payload", payload)])
            .await;
    });
    HttpResponse::Ok()
}

#[get("/messages/{msg}/mark_as_read")]
async fn mark_as_read(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    path: Path<String>,
) -> impl Responder {
    let msg = path.into_inner();
    let event = Receipt {
        message_id: msg,
        user_id: claims.sub.clone(),
        delivered: false,
        read: false,
        reaction: None,
        ts: chrono::Utc::now().timestamp(),
    };
    let redis = state.redis.clone();
    rt::spawn(async move {
        let mut conn = match redis.get_async_connection().await {
            Ok(c) => c,
            Err(_) => return,
        };

        let payload = serde_json::to_string(&event).unwrap();

        let _: redis::RedisResult<String> = conn
            .xadd("receipts_stream", "*", &[("payload", payload)])
            .await;
    });
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
    let redis = create_redis();
    let redis_w1 = redis.clone();
    let redis_w2 = redis.clone();
    let worker_d = db.clone();
    let db_w2 = db.clone();
    tokio::spawn(async move { db_worker(&redis_w1, &worker_d).await });
    tokio::spawn(async move { receipt_worker(&redis_w2, &db_w2).await });
    let state = web::Data::new(AppState {
        db,
        chat_server,
        redis,
    });

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
                .service(get_receipts)
                .service(mark_as_read)
                .service(list_conversations)
                .service(ws_route),
        )
    })
    .workers(12)
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
