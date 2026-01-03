use sqlx::{SqlitePool, SqliteConnection, query, query_as, Error};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::models::{ConversationResponse,MessageResponse, Participant as PModel};

pub struct MessageReceipt{}
pub struct Message{}
pub struct Conversation{}
pub struct Participant{}

fn time_now()->i64{
    SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64
}


impl MessageReceipt{
    pub async fn create(db: &SqlitePool, msg: i64, user:&String){
        let now = time_now();
        let _ = query(
        r#"
        INSERT INTO message_receipts (message, user, delivered_at)
        VALUES (?, ?, ?)
        ON CONFLICT(message, user)
        DO UPDATE SET
            delivered_at = COALESCE(delivered_at, excluded.delivered_at)
        "#
    )
    .bind(msg)
    .bind(user)
    .bind(now)
    .execute(db)
    .await
        .ok();
    }
    
    pub async fn create_table(db: &SqlitePool){
        query(
        r#"
        CREATE TABLE IF NOT EXISTS message_receipts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message INTEGER NOT NULL,
    user TEXT NOT NULL,
    delivered_at INTEGER,
    read_at INTEGER,
    UNIQUE(message, user)
)
        "#,
    )
    .execute(db)
    .await;
    }
}

impl Message{
    pub async fn create_table(db: &SqlitePool){
        query(
        r#"
        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source TEXT NOT NULL,
            conversation TEXT NOT NULL,
            text TEXT NOT NULL,
            created INTEGER NOT NULL
        )
        "#,
    )
    .execute(db)
    .await;
    }
    
    pub async fn retrieve(db: &SqlitePool, conversation: &String, limit:i64)->Result<Vec<MessageResponse>,Error>{
        query_as::<_, MessageResponse>(
                r#"
                SELECT id, conversation, source, text, created
                FROM messages
                WHERE conversation = ?
                ORDER BY created DESC
                LIMIT ?
                "#
            )
            .bind(conversation)
            .bind(limit)
            .fetch_all(db)
            .await
    }
    pub async fn insert(db: &SqlitePool, conversation: &String, source: &String, text: &String)->Result<MessageResponse,Error>{
        let created = time_now();
        
        sqlx::query_as::<_, MessageResponse>(
        r#"
        INSERT INTO messages ( conversation, source, text, created)
        VALUES ( ?, ?, ?, ?)
        RETURNING id, conversation, source, text, created
        "#
    )
    .bind(conversation)
    .bind(source)
    .bind(text)
    .bind(created)
    .fetch_one(db)
    .await
    }
}

impl Conversation{
    pub async fn create_table(db: &SqlitePool){
        query(
        r#"
        CREATE TABLE IF NOT EXISTS conversations (
            name TEXT PRIMARY KEY,
            admin TEXT NOT NULL,
    title TEXT,
            created INTEGER NOT NULL
        )
        "#,
    )
    .execute(db)
    .await;
    }
    pub async fn insert(tx: &mut SqliteConnection,conversation_name: &str, title: &Option<String>, admin: &str)->Result<ConversationResponse, Error>{
        let created = time_now();
        query_as::<_, ConversationResponse>(
        r#"
        INSERT INTO conversations (name, title, admin, created)
        VALUES (?, ?, ?, ?)
        RETURNING title, name, admin, created
        "#,
    )
    .bind(conversation_name)
        .bind(title)
    .bind(admin)
    .bind(created)
    .fetch_one(tx)
        .await
    }
}

impl Participant{
    pub async fn create_table(db: &SqlitePool){
        query(
        r#"
        CREATE TABLE IF NOT EXISTS participants (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation TEXT NOT NULL,
            participant TEXT NOT NULL,
            created INTEGER NOT NULL,
    UNIQUE(conversation, participant)
        )
        "#,
    )
    .execute(db)
    .await;
    }
    pub async fn insert(tx: &mut SqliteConnection, conversation_name: &String, subject: &String)->Result<(), Error>{
        let created = time_now();
        query(
        r#"
        INSERT INTO participants (conversation, participant, created)
        VALUES (?, ?, ?)
        "#,
    )
    .bind(conversation_name)
    .bind(subject)
    .bind(created)
    .execute(tx)
        .await;
        
        Ok(())
    }
    pub async fn retrieve(db: &SqlitePool,conversation: &String)->Result<Vec<PModel>, Error>{
        query_as::<_,PModel>(
        r#"
        SELECT id, conversation, participant, created FROM participants WHERE conversation = ?
        "#
        )
        .bind(conversation)
        .fetch_all(db)
        .await
    }
}