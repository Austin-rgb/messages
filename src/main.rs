use actix::{Actor, Addr};
use actix_web::{App, HttpServer, web, web::scope};
use auth_middleware::Auth;
use dotenvy::dotenv;

use sqlx::{SqlitePool, query};
use std::vec::Vec;

mod libcache;
mod libworkers;
mod logging;
mod models;
mod redis_cfg;
mod repo;
mod repositories;
mod workers;
mod ws;
use crate::logging::LoggingMiddleware;
use crate::repositories::{Conversation, Message, MessageReceipt, Participant as PRepo};
use crate::workers::db_worker;
use crate::ws::{ChatServer, DeliverMessage, ws_route};
use crate::{config::config, redis_cfg::create_redis};
use crate::{
    models::{AppState, InsertMessage},
    workers::receipt_worker,
};
mod config;
mod handlers;

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
    //env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    HttpServer::new(move || {
        App::new().app_data(state.clone()).service(
            scope("")
                //.wrap(LoggingMiddleware)
                .wrap(Auth)
                .service(ws_route)
                .configure(config),
        )
    })
    .workers(12)
    .bind(("127.0.0.1", 8082))?
    .run()
    .await
}
