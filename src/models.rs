use crate::ws::ChatServer;
use actix::Addr;
use redis::Client;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, Clone, FromRow, Deserialize)]
pub struct InsertMessage {
    pub source: String,
    pub conversation: String,
    pub text: String,
    pub reply_to: Option<i64>,
    pub created: i64,
    pub id: String,
}

pub struct AppState {
    pub db: SqlitePool,
    pub chat_server: Addr<ChatServer>,
    pub redis: Client,
}

#[derive(Deserialize, Clone)]
pub struct MessageFilters {
    pub source: Option<String>,
    pub text: Option<String>,
    pub created: Option<i64>,
    pub reply_to: Option<i64>,
    pub limit: Option<i32>,  // max items per page
    pub offset: Option<i32>, // items to skip
}

impl MessageFilters {
    pub fn limit(&self) -> i32 {
        self.limit.unwrap_or(1000) // default 20 items
    }

    pub fn offset(&self) -> i32 {
        self.offset.unwrap_or(0) // default start from 0
    }
}

#[derive(Deserialize)]
pub struct CreateMessage {
    pub text: String, // message content
    pub reply_to: Option<i64>,
}
#[derive(Serialize, FromRow)]
pub struct ConversationListItem {
    name: String,
    title: Option<String>,
    admin: String,
    created: i64,
}

#[derive(Deserialize)]
pub struct CreateConversation {
    pub participants: Vec<String>,
    pub name: Option<String>,
    pub title: Option<String>,
}
#[derive(Serialize, sqlx::FromRow)]
pub struct ConversationResponse {
    pub name: String,
    pub admin: String,
    pub title: Option<String>,
    pub created: i64,
}

#[derive(FromRow, Clone, Serialize, Deserialize)]
pub struct Participant {
    id: i64,
    conversation: String,
    pub participant: String,
    created: i64,
}

#[derive(Deserialize)]
pub struct ConversationQuery {
    user: String,
}

#[derive(Serialize, FromRow)]
pub struct MessageReceipt {
    user: String,
    delivered_at: i64,
    read_at: i64,
    reaction: i64,
}

#[derive(Serialize, Deserialize)]
pub struct Receipt {
    pub message: String,
    pub user: String,
    pub delivered: bool,
    pub read: bool,
    pub reaction: Option<i64>,
}

#[derive(FromRow, Serialize)]
pub struct RetrieveReceipt {
    pub message: String,
    pub user: String,
    pub delivered_at: Option<i64>,
    pub read_at: Option<i64>,
    pub reaction: Option<i64>,
}
