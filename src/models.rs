use serde::{Deserialize, Serialize};
use actix::Addr;
use sqlx::{SqlitePool,FromRow};
use crate::ws::ChatServer;
fn default_limit() -> i64 {
    50
}

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
}

#[derive(Deserialize)]
pub struct CreateMessage {
    pub text: String,     // message content
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
struct Message {
    id: i64,
    source: String,
    conversation: String,
    text: String,
    created: i64,
}

#[derive(FromRow)]
struct Conversation {
    name:String,
    title:Option<String>,
    admin: String,
    created: i64,
}

#[derive(FromRow)]
struct DeliveryNote {
    id: i64,
    message: i64,
    dest: String,
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

#[derive(Deserialize)]
pub struct GetMessagesQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub before: Option<i64>,
}