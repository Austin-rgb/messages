use actix::{Actor, Addr};
use actix_web::{App, HttpServer, web, web::scope};
use auth_middleware::Auth;
use dotenvy::dotenv;

use sqlx::{SqlitePool, query};
use std::vec::Vec;

mod logging;
mod models;
mod repositories;
mod workers;
mod ws;
use crate::config::config;
use crate::models::{AppState, InsertMessage};
use crate::models::{Receipt, Workers};
use crate::repositories::{Box, Conversation, Message, MessageReceipt, Participant as PRepo};
use crate::workers::{IMHandler, ReceiptHandler};
use crate::ws::{ChatServer, DeliverMessage, ws_route};
use libworkers::{BatchWorker, MpscQueue};
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
    Box::create_table(db).await;

    // Add indexes
    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_messages_conversations 
        ON messages(mbox, created)
        "#,
    )
    .execute(db)
    .await
    .unwrap();

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_participants_conversation 
        ON participants(conversation)
        "#,
    )
    .execute(db)
    .await
    .unwrap();

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_participants_user 
        ON participants(participant)
        "#,
    )
    .execute(db)
    .await
    .unwrap();

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
    let worker_d = db.clone();

    let mut msg_writer: MpscQueue<InsertMessage> = MpscQueue::new();
    let msg_worker = msg_writer.sender.clone();
    let mut rt_writer: MpscQueue<Receipt> = MpscQueue::new();
    tokio::spawn(async move { msg_writer.start_worker(worker_d, 100, IMHandler {}).await });

    let value = db.clone();
    let receipt_worker = rt_writer.sender.clone();
    tokio::spawn(async move { rt_writer.start_worker(value, 100, ReceiptHandler {}).await });
    let state = web::Data::new(AppState {
        db,
        chat_server,
        workers: Workers {
            msg_worker,
            receipt_worker,
        },
    });
    //env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    //

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
