use actix_web::{HttpResponse, Responder, get, post, rt, web, web::Path};
use auth_middleware::UserContext;
use once_cell::sync::Lazy;

use crate::models::{Box as BoxModel, InsertConversation};
use crate::repositories;
use crate::repositories::is_sender;
use crate::repositories::{
    Conversation, MessageReceipt, Participant as PRepo, is_participant, time_now,
};
use crate::ws::DeliverMessage;
use crate::{
    deliver_message,
    models::{
        AppState, ConversationListItem, ConversationResponse, CreateConversation, CreateMessage,
        InsertMessage, MessageFilters, Participant, Receipt,
    },
};
use libworkers::{Cache, CacheError, LocalCache};
use repositories::Box;
use serde_json::to_string;
use sqlx::{SqlitePool, query_as};
use std::vec::Vec;
use uuid::Uuid;

async fn get_default_mbox(db: &SqlitePool, peer: String) -> Result<String, CacheError> {
    MBOX_CACHE
        .get(&peer, async || {
            let mbox = query_as::<_, BoxModel>(r#"select * from boxes where owner=?"#)
                .bind(peer.clone())
                .fetch_one(db)
                .await;
            match mbox {
                Ok(mb) => Ok(mb.id),
                Err(e) => match e {
                    sqlx::Error::RowNotFound => Ok(create_default_mbox(db, peer.clone()).await),
                    _ => return Err(CacheError::Fallback),
                },
            }
        })
        .await
}

async fn create_default_mbox(db: &SqlitePool, owner: String) -> String {
    let mbox = BoxModel {
        id: Uuid::new_v4().to_string(),
        owner,
        kind: 0,
    };
    let _ = Box::insert(db, mbox.clone()).await;
    mbox.id
}

pub static PARTICIPANTS_CACHE: Lazy<LocalCache<Vec<Participant>>> = Lazy::new(|| {
    LocalCache::new(600) // TTL = 60s
});

pub static MBOX_CACHE: Lazy<LocalCache<String>> = Lazy::new(|| {
    LocalCache::new(600) // TTL = 60s
});

#[post("/conversations")]
async fn create_conversation(
    state: web::Data<AppState>,
    claims: web::ReqData<UserContext>,
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

    let name = Uuid::new_v4().to_string();

    // 1️⃣ Create conversation (admin = creator)

    let conv = InsertConversation {
        name,
        admin: claims.username.clone(),
        title: payload.title.clone(),
    };

    let conversation = match Conversation::insert(&mut *tx, &conv).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Conversation insert error: {:?}", e);
            return HttpResponse::InternalServerError().finish();
        }
    };

    // 2️⃣ Insert creator as participant
    if let Err(e) = PRepo::insert(&mut *tx, &conversation.name, &claims.username).await {
        eprintln!("Participant insert error: {:?}", e);
        return HttpResponse::InternalServerError().finish();
    }

    // 3️⃣ Insert other participants (deduplicated)
    let participants: Vec<String> = payload
        .participants
        .iter()
        .filter(|u| *u != &claims.username)
        .cloned()
        .collect();

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
    claims: web::ReqData<UserContext>,
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
    .bind(&claims.username)
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
    claims: web::ReqData<UserContext>,
    path: Path<String>,
) -> impl Responder {
    let conversation_name = path.into_inner();

    // Check if user is a participant

    if !is_participant(&state.db, &conversation_name, &claims.username).await {
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

#[get("/inbox/messages")]
async fn get_pmessages(
    state: web::Data<AppState>,
    claims: web::ReqData<UserContext>,
    query: web::Query<MessageFilters>,
) -> impl Responder {
    let query = query.into_inner();
    let mbox = match get_default_mbox(&state.db, claims.username.clone()).await {
        Ok(mb) => mb,
        Err(e) => {
            let ev: Vec<InsertMessage> = Vec::new();
            return HttpResponse::Ok().json(ev);
        }
    };
    let messages: Vec<InsertMessage> =
        match repositories::Message::retrieve(&state.db, &mbox, query).await {
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
            message: msg.id.clone(),
            user: claims.username.clone(),
            delivered: true,
            read: false,
            reaction: None,
        })
        .collect();
    for event in events {
        let _ = state.workers.receipt_worker.send(event).await;
    }
    HttpResponse::Ok().json(messages)
}

#[post("/inbox/{peer}/messages")]
async fn peer_message(
    state: web::Data<AppState>,
    claims: web::ReqData<UserContext>,
    path: web::Path<String>,
    payload: web::Json<CreateMessage>,
) -> impl Responder {
    let peer = path.into_inner();
    let mbox = match get_default_mbox(&state.db, peer.clone()).await {
        Ok(mb) => mb,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    let msg = InsertMessage {
        source: claims.username.clone(),
        mbox: mbox,
        text: payload.text.clone(),
        reply_to: payload.reply_to,
        created: time_now(),
        id: Uuid::new_v4().to_string(),
    };
    let _ = state.workers.msg_worker.send(msg.clone()).await;

    let payload = match to_string(&msg) {
        Ok(p) => p,
        Err(e) => return HttpResponse::from_error(e),
    };
    state.chat_server.do_send(DeliverMessage {
        to: peer,
        payload: payload,
        id: Uuid::new_v4().to_string(),
    });
    HttpResponse::Ok().finish()
}

#[post("/conversations/{name}/messages")]
async fn post_message(
    state: web::Data<AppState>,
    claims: web::ReqData<UserContext>,
    path: Path<String>,
    payload: web::Json<CreateMessage>,
) -> impl Responder {
    let conversation_name = path.into_inner();
    // 1️⃣ Get participants from cache or fallback to DB
    let participants: Vec<Participant> = match PARTICIPANTS_CACHE
        .get(&conversation_name, || async {
            // fallback closure if cache miss
            repositories::Participant::retrieve(&state.db, &conversation_name, 1000, 0)
                .await
                .map_err(|e| CacheError::Fallback)
        })
        .await
    {
        Ok(v) => v,
        Err(_) => todo!(),
    };

    if !participants
        .iter()
        .any(|p| p.participant == claims.username)
    {
        return HttpResponse::Forbidden().body("Not a participant in this conversation");
    }

    // Insert message
    let bus = state.chat_server.clone();

    let msg = InsertMessage {
        source: claims.username.clone(),
        mbox: conversation_name,
        text: payload.text.clone(),
        reply_to: payload.reply_to,
        created: time_now(),
        id: Uuid::new_v4().to_string(),
    };

    let _ = state.workers.msg_worker.send(msg.clone()).await;
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
    claims: web::ReqData<UserContext>,
    path: Path<String>,
    query: web::Query<MessageFilters>,
) -> impl Responder {
    let conversation_name = path.into_inner();
    let query = query.into_inner();
    // Check if user is a participant

    if !is_participant(&state.db, &conversation_name, &claims.username).await {
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
            message: msg.id.clone(),
            user: claims.username.clone(),
            delivered: true,
            read: false,
            reaction: None,
        })
        .collect();
    for event in events {
        let _ = state.workers.receipt_worker.send(event).await;
    }
    HttpResponse::Ok().json(messages)
}

#[get("/messages/{msg}/receipts")]
async fn get_receipts(
    state: web::Data<AppState>,
    claims: web::ReqData<UserContext>,
    path: Path<String>,
) -> impl Responder {
    // confirm message ownership
    let msg = path.into_inner();
    if !is_sender(&state.db, &msg, &claims.username).await {
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
    claims: web::ReqData<UserContext>,
    path: Path<(String, i64)>,
) -> impl Responder {
    let (msg, reaction) = path.into_inner();
    let event = Receipt {
        message: msg,
        user: claims.username.clone(),
        delivered: false,
        read: false,
        reaction: Some(reaction),
    };
    let _ = state.workers.receipt_worker.send(event).await;
    HttpResponse::Ok()
}

#[get("/messages/{msg}/mark_as_read")]
async fn mark_as_read(
    state: web::Data<AppState>,
    claims: web::ReqData<UserContext>,
    path: Path<String>,
) -> impl Responder {
    let msg = path.into_inner();
    let event = Receipt {
        message: msg,
        user: claims.username.clone(),
        delivered: false,
        read: false,
        reaction: None,
    };
    let _ = state.workers.receipt_worker.send(event).await;
    HttpResponse::Ok()
}
