use crate::models::{
    ConversationResponse, MessageFilters, MessageReceipt as Receipt, MessageResponse,
    Participant as PModel,
};
use sqlx::{Error, QueryBuilder, Sqlite, SqliteConnection, SqlitePool, query, query_as};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct MessageReceipt {}
pub struct Message {}
pub struct Conversation {}
pub struct Participant {}

fn time_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

pub async fn is_participant(db: &SqlitePool, conversation: &String, user: &String) -> bool {
    sqlx::query_scalar(
        r#"SELECT EXISTS(
            SELECT 1 FROM participants 
            WHERE conversation = ? AND participant = ?
        )"#,
    )
    .bind(conversation)
    .bind(user)
    .fetch_one(db)
    .await
    .unwrap_or(false)
}

impl MessageReceipt {
    pub async fn create(
        db: &SqlitePool,
        msg: i64,
        user: &String,
        delivered: bool,
        read: bool,
        reaction: Option<i64>,
    ) {
        let now = time_now();
        let delivered_at: Option<i64> = if delivered { Some(now) } else { None };
        let read_at: Option<i64> = if read { Some(now) } else { None };
        let _ = query(
            r#"
        INSERT INTO message_receipts (message, user, delivered_at, read_at, reaction)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(message, user)
        DO UPDATE SET
            delivered_at = COALESCE(delivered_at, excluded.delivered_at)
        read_at = COALESCE(read_at, excluded.read_at)
        reaction = excluded.reaction
        "#,
        )
        .bind(msg)
        .bind(user)
        .bind(delivered_at)
        .bind(read_at)
        .bind(reaction)
        .execute(db)
        .await
        .ok();
    }

    pub async fn retrieve(db: &SqlitePool, msg: i64) -> Result<Vec<Receipt>, Error> {
        query_as::<_, Receipt>(
            r#"
        SELECT user, delivered_at, read_at, reaction FROM message_receipts WHERE message = ?
        "#,
        )
        .bind(msg)
        .fetch_all(db)
        .await
    }

    pub async fn create_table(db: &SqlitePool) {
        let _ = query(
            r#"
        CREATE TABLE IF NOT EXISTS message_receipts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message INTEGER NOT NULL,
    user TEXT NOT NULL,
    delivered_at INTEGER,
    read_at INTEGER,
        reaction INTEGER,
    UNIQUE(message, user)
        )
        "#,
        )
        .execute(db)
        .await;
    }
}

impl Message {
    pub fn build_filters(
        mut qb: QueryBuilder<Sqlite>,
        query: MessageFilters,
    ) -> QueryBuilder<Sqlite> {
        if let Some(created) = query.created {
            qb.push(" AND created = ");
            qb.push_bind(created);
        }
        if let Some(reply_to) = query.reply_to {
            qb.push(" AND reply_to = ");
            qb.push_bind(reply_to);
        } else if let Some(source) = query.source {
            qb.push(" AND source = ");
            qb.push_bind(source);
        }
        qb
    }
    pub async fn create_table(db: &SqlitePool) {
        let _ = query(
            r#"
        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source TEXT NOT NULL,
            conversation TEXT NOT NULL,
            text TEXT NOT NULL,
            reply_to INTEGER,
            created INTEGER NOT NULL
        )
        "#,
        )
        .execute(db)
        .await;
    }

    pub async fn retrieve(
        db: &SqlitePool,
        conversation: &String,
        query: MessageFilters,
    ) -> Result<Vec<MessageResponse>, Error> {
        let mut qb = QueryBuilder::new(
            "SELECT id, conversation, source, text, created, reply_to
                FROM messages WHERE 1=1",
        );
        qb = Message::build_filters(qb, query.clone());
        qb.push(" AND conversation = ");
        qb.push_bind(conversation);
        qb.push(" LIMIT ");
        qb.push_bind(query.clone().limit());
        qb.push(" OFFSET ");
        qb.push_bind(query.offset());
        qb.build_query_as::<MessageResponse>().fetch_all(db).await
    }

    pub async fn insert(
        db: &SqlitePool,
        conversation: &String,
        source: &String,
        text: &String,
        reply_to: &Option<i64>,
    ) -> Result<MessageResponse, Error> {
        let created = time_now();

        sqlx::query_as::<_, MessageResponse>(
            r#"
        INSERT INTO messages ( conversation, source, text, created, reply_to)
        VALUES ( ?, ?, ?, ?, ?)
        RETURNING id, conversation, source, text, created, reply_to
        "#,
        )
        .bind(conversation)
        .bind(source)
        .bind(text)
        .bind(created)
        .bind(reply_to)
        .fetch_one(db)
        .await
    }
}

impl Conversation {
    pub async fn create_table(db: &SqlitePool) {
        let _ = query(
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
    pub async fn insert(
        tx: &mut SqliteConnection,
        conversation_name: &str,
        title: &Option<String>,
        admin: &str,
    ) -> Result<ConversationResponse, Error> {
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

impl Participant {
    pub async fn create_table(db: &SqlitePool) {
        let _ = query(
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
    pub async fn insert(
        tx: &mut SqliteConnection,
        conversation_name: &String,
        subject: &String,
    ) -> Result<(), Error> {
        let created = time_now();
        let _ = query(
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
    pub async fn retrieve(
        db: &SqlitePool,
        conversation: &String,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<PModel>, Error> {
        query_as::<_,PModel>(
        r#"
        SELECT id, conversation, participant, created FROM participants WHERE conversation = ? LIMIT ? OFFSET ?
        "#
        )
        .bind(conversation)
        .bind(limit)
        .bind(offset)
        .fetch_all(db)
        .await
    }
}
