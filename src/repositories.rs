use crate::models::{
    Box as BoxModel, ConversationResponse, InsertConversation, InsertMessage, MessageFilters,
    Participant as PModel, Receipt, RetrieveReceipt,
};
use sqlx::{
    Error, Pool, QueryBuilder, Sqlite, SqliteConnection, SqlitePool, query, query_as,
    sqlite::SqliteQueryResult,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct MessageReceipt {
    pub user_id: (),
}

pub struct Message {}
pub struct Conversation {}
pub struct Participant {}

pub fn time_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(t) => t.as_millis() as i64,
        Err(e) => {
            eprintln!("{}", e);
            panic!();
        }
    }
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

pub async fn is_sender(db: &SqlitePool, id: &String, user: &String) -> bool {
    sqlx::query_scalar(
        r#"SELECT EXISTS(
            SELECT 1 FROM messages
            WHERE id = ? AND source = ?
        )"#,
    )
    .bind(id)
    .bind(user)
    .fetch_one(db)
    .await
    .unwrap_or(false)
}

impl MessageReceipt {
    pub async fn upsert(db: &mut SqliteConnection, receipt: &Receipt) {
        let now = time_now();
        let delivered_at: Option<i64> = if receipt.delivered { Some(now) } else { None };
        let read_at: Option<i64> = if receipt.read { Some(now) } else { None };
        let _ = query(
            r#"
        INSERT INTO message_receipts (message, user, delivered_at, read_at, reaction)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(message, user)
        DO UPDATE SET
            delivered_at = COALESCE(delivered_at, excluded.delivered_at),
        read_at = COALESCE(read_at, excluded.read_at),
        reaction = excluded.reaction
        "#,
        )
        .bind(receipt.message.clone())
        .bind(receipt.user.clone())
        .bind(delivered_at)
        .bind(read_at)
        .bind(receipt.reaction)
        .execute(db)
        .await;
    }

    pub async fn retrieve(db: &SqlitePool, msg: String) -> Result<Vec<RetrieveReceipt>, Error> {
        query_as::<_, RetrieveReceipt>(
            r#"
        SELECT  message, user, delivered_at, read_at, reaction FROM message_receipts WHERE message = ?
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

    pub async fn batch_upsert(
        db: &mut SqliteConnection,
        receipts: &[Receipt],
    ) -> Result<(), sqlx::Error> {
        if receipts.is_empty() {
            return Ok(());
        }

        let now = time_now();

        // Start building the query
        let mut qb = QueryBuilder::<Sqlite>::new(
            r#"
        INSERT INTO message_receipts (message, user, delivered_at, read_at, reaction)
        "#,
        );

        // Add VALUES
        qb.push_values(receipts, |mut b, r| {
            let delivered_at: Option<i64> = if r.delivered { Some(now) } else { None };
            let read_at: Option<i64> = if r.read { Some(now) } else { None };

            b.push_bind(&r.message)
                .push_bind(&r.user)
                .push_bind(delivered_at)
                .push_bind(read_at)
                .push_bind(r.reaction);
        });

        // Add ON CONFLICT clause
        qb.push(
            r#"
        ON CONFLICT(message, user)
        DO UPDATE SET
            delivered_at = COALESCE(message_receipts.delivered_at, excluded.delivered_at),
            read_at = COALESCE(message_receipts.read_at, excluded.read_at),
            reaction = excluded.reaction
        "#,
        );

        // Execute
        qb.build().execute(db).await?;

        Ok(())
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
            id TEXT NOT NULL,
            source TEXT NOT NULL,
            mbox TEXT NOT NULL,
            text TEXT NOT NULL,
            reply_to INTEGER,
            created INTEGER NOT NULL,
            UNIQUE(id)
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
    ) -> Result<Vec<InsertMessage>, Error> {
        let mut qb = QueryBuilder::new(
            "SELECT id,mbox, source, text, created, reply_to
                FROM messages WHERE 1=1",
        );
        qb = Message::build_filters(qb, query.clone());
        qb.push(" AND mbox = ");
        qb.push_bind(conversation);
        qb.push(" LIMIT ");
        qb.push_bind(query.clone().limit());
        qb.push(" OFFSET ");
        qb.push_bind(query.offset());
        qb.build_query_as::<InsertMessage>().fetch_all(db).await
    }

    pub async fn insert(db: &SqlitePool, msg: InsertMessage) -> Result<InsertMessage, Error> {
        sqlx::query_as::<_, InsertMessage>(
            r#"
        INSERT INTO messages ( id,mbox source, text, created, reply_to)
        VALUES ( ?, ?, ?, ?, ?)
        RETURNING id,mbox source, text, created, reply_to
        "#,
        )
        .bind(msg.id)
        .bind(msg.mbox)
        .bind(msg.source)
        .bind(msg.text)
        .bind(msg.created)
        .bind(msg.reply_to)
        .fetch_one(db)
        .await
    }
    pub async fn insert_many(
        db: &SqlitePool,
        msgs: Vec<InsertMessage>,
    ) -> Result<SqliteQueryResult, Error> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            "INSERT INTO messages ( id, mbox, source, text, created, reply_to)",
        );
        qb.push_values(msgs, |mut b, user| {
            b.push_bind(user.id)
                .push_bind(user.mbox)
                .push_bind(user.source)
                .push_bind(user.text)
                .push_bind(user.created)
                .push_bind(user.reply_to);
        });
        qb.build().execute(db).await
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
            created INTEGER NOT NULL,
            mbox TEXT NOT NULL
        )
        "#,
        )
        .execute(db)
        .await;
    }
    pub async fn insert(
        tx: &mut SqliteConnection,
        conv: &InsertConversation,
    ) -> Result<ConversationResponse, Error> {
        let created = time_now();
        query_as::<_, ConversationResponse>(
            r#"
        INSERT INTO conversations (name, title, admin, created)
        VALUES (?, ?, ?, ?)
        RETURNING title, name, admin, created
        "#,
        )
        .bind(conv.name.clone())
        .bind(conv.title.clone())
        .bind(conv.admin.clone())
        .bind(created)
        .fetch_one(tx)
        .await
    }
}

pub struct Box {}

impl Box {
    pub async fn create_table(db: &SqlitePool) {
        let _ = query(
            r#"
        CREATE TABLE IF NOT EXISTS boxes (
            id TEXT PRIMARY KEY,
            owner TEXT NOT NULL,
    title TEXT,
            kind INTEGER NOT NULL
        )
        "#,
        )
        .execute(db)
        .await;
    }

    pub async fn insert(
        tx: &Pool<Sqlite>,
        msgbox: BoxModel,
    ) -> Result<ConversationResponse, Error> {
        query_as::<_, ConversationResponse>(
            r#"
        INSERT INTO boxes (id, owner, kind)
        VALUES (?, ?, ?)
        RETURNING title, name, admin, created
        "#,
        )
        .bind(msgbox.id)
        .bind(msgbox.owner)
        .bind(msgbox.kind as u32)
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

    pub async fn insert_many(
        tx: &mut SqliteConnection,
        conversation_name: &String,
        subjects: Vec<String>,
    ) -> Result<(), Error> {
        let created = time_now();
        let mut qb = QueryBuilder::new(
            "INSERT OR IGNORE INTO participants (conversation, participant, created) ",
        );

        qb.push_values(subjects, |mut b, subject| {
            b.push_bind(conversation_name)
                .push_bind(subject)
                .push_bind(created);
        });

        qb.build().execute(tx).await?;
        Ok(())
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
