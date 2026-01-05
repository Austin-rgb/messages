use serde::{Deserialize, Serialize};
use actix::Addr;
use sqlx::{SqlitePool,FromRow};
use crate::ws::ChatServer;


pub struct AppState {
    pub db: SqlitePool,
    pub chat_server:Addr<ChatServer>,
}

#[derive(Serialize, FromRow, Clone)]
pub struct MessageResponse {
    pub id: i64,
    pub conversation: String,
    pub source: String,
    pub text: String,
    pub created: i64,
    pub reply_to: Option<i64>,
}

#[derive(Deserialize, Clone)]
pub struct MessageFilters {
    pub source: Option<String>,
    pub text: Option<String>,
    pub created: Option<i64>,
    pub reply_to: Option<i64>,
    pub limit: Option<i32>,   // max items per page
    pub offset: Option<i32>,  // items to skip
}

impl MessageFilters{
    pub fn limit(&self) -> i32 {
        self.limit.unwrap_or(20) // default 20 items
    }

    pub fn offset(&self) -> i32 {
        self.offset.unwrap_or(0) // default start from 0
    }
}

#[derive(Deserialize)]
pub struct CreateMessage {
    pub text: String,     // message content
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
    pub name:Option<String>,
    pub title:Option<String>
}
#[derive(Serialize, sqlx::FromRow)]
pub struct ConversationResponse {
    pub name:String,
    pub admin: String,
    pub title:Option<String>,
    pub created: i64,
}


#[derive(FromRow)]
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
pub struct MessageReceipt{
    user:i64,
    delivered_at:i64,
    read_at:i64,
    reaction:i64
}
