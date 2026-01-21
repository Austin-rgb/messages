// Robust repository layer for messaging domain using sqlx + SQLite
// Focus: transactional safety, strong typing, FK integrity, and composable APIs

use sqlx::{Error, QueryBuilder, Sqlite, SqliteConnection, SqlitePool, query, query_as};
use std::time::{SystemTime, UNIX_EPOCH};

// --------------------
// Utilities
// --------------------

pub fn time_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// --------------------
// Repository Traits
// --------------------

pub trait ConversationRepository {
    fn pool(&self) -> &SqlitePool;
}

pub trait MessageRepository {
    fn pool(&self) -> &SqlitePool;
}

pub trait ParticipantRepository {
    fn pool(&self) -> &SqlitePool;
}

pub trait ReceiptRepository {
    fn pool(&self) -> &SqlitePool;
}

// --------------------
// Concrete Repositories
// --------------------

pub struct ConversationRepo {
    pub db: SqlitePool,
}

pub struct MessageRepo {
    pub db: SqlitePool,
}

pub struct ParticipantRepo {
    pub db: SqlitePool,
}

pub struct ReceiptRepo {
    pub db: SqlitePool,
}

impl ConversationRepository for ConversationRepo {
    fn pool(&self) -> &SqlitePool {
        &self.db
    }
}

impl MessageRepository for MessageRepo {
    fn pool(&self) -> &SqlitePool {
        &self.db
    }
}

impl ParticipantRepository for ParticipantRepo {
    fn pool(&self) -> &SqlitePool {
        &self.db
    }
}

impl ReceiptRepository for ReceiptRepo {
    fn pool(&self) -> &SqlitePool {
        &self.db
    }
}

// --------------------
// Conversation API
// --------------------

impl ConversationRepo {
    pub async fn create_table(&self) -> Result<(), Error> {
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
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn insert(
        &self,
        tx: &mut SqliteConnection,
        name: &str,
        title: Option<&str>,
        admin: &str,
    ) -> Result<(), Error> {
        let created = time_now();
        query(
            r#"
            INSERT INTO conversations (name, title, admin, created)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(name)
        .bind(title)
        .bind(admin)
        .bind(created)
        .execute(tx)
        .await?;
        Ok(())
    }
}

// --------------------
// Participant API
// --------------------

impl ParticipantRepo {
    /// High-throughput batch participant insert
    pub async fn insert_many_fast(
        &self,
        tx: &mut SqliteConnection,
        conversation: &str,
        users: &[String],
    ) -> Result<(), Error> {
        const CHUNK: usize = 300;
        let created = time_now();

        for batch in users.chunks(CHUNK) {
            let mut qb = QueryBuilder::<Sqlite>::new(
                "INSERT OR IGNORE INTO participants (conversation, participant, created) ",
            );

            qb.push_values(batch, |mut b, user| {
                b.push_bind(conversation).push_bind(user).push_bind(created);
            });

            qb.build().execute(&mut *tx).await?;
        }

        Ok(())
    }

    pub async fn create_table(&self) -> Result<(), Error> {
        query(
            r#"
            CREATE TABLE IF NOT EXISTS participants (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation TEXT NOT NULL,
                participant TEXT NOT NULL,
                created INTEGER NOT NULL,
                UNIQUE(conversation, participant),
                FOREIGN KEY(conversation) REFERENCES conversations(name) ON DELETE CASCADE
            )
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn insert_many(
        &self,
        tx: &mut SqliteConnection,
        conversation: &str,
        users: &[String],
    ) -> Result<(), Error> {
        let created = time_now();
        let mut qb = QueryBuilder::<Sqlite>::new(
            "INSERT OR IGNORE INTO participants (conversation, participant, created) ",
        );

        qb.push_values(users, |mut b, user| {
            b.push_bind(conversation).push_bind(user).push_bind(created);
        });

        qb.build().execute(tx).await?;
        Ok(())
    }

    pub async fn is_participant(&self, conversation: &str, user: &str) -> Result<bool, Error> {
        let v: i64 = sqlx::query_scalar(
            r#"SELECT EXISTS(
                SELECT 1 FROM participants
                WHERE conversation = ? AND participant = ?
            )"#,
        )
        .bind(conversation)
        .bind(user)
        .fetch_one(self.pool())
        .await?;

        Ok(v != 0)
    }
}

// --------------------
// Message API
// --------------------

// NOTE: Background workers should prefer batch inserts with chunking + single transaction

impl MessageRepo {
    /// Batch insert optimized for background workers
    /// - Single transaction
    /// - Chunked to avoid SQLite bind limits
    pub async fn insert_many(
        &self,
        tx: &mut SqliteConnection,
        messages: &[(&str, &str, &str, &str, Option<&str>)],
    ) -> Result<(), Error> {
        const CHUNK: usize = 200; // safe for SQLite variable limits

        for batch in messages.chunks(CHUNK) {
            let mut qb = QueryBuilder::<Sqlite>::new(
                "INSERT INTO messages (id, conversation, source, text, created, reply_to) ",
            );

            let created = time_now();

            qb.push_values(batch, |mut b, (id, conv, src, text, reply)| {
                b.push_bind(id)
                    .push_bind(conv)
                    .push_bind(src)
                    .push_bind(text)
                    .push_bind(created)
                    .push_bind(reply);
            });

            qb.build().execute(&mut *tx).await?;
        }

        Ok(())
    }

    pub async fn create_table(&self) -> Result<(), Error> {
        query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                conversation TEXT NOT NULL,
                text TEXT NOT NULL,
                reply_to TEXT,
                created INTEGER NOT NULL,
                FOREIGN KEY(conversation) REFERENCES conversations(name) ON DELETE CASCADE,
                FOREIGN KEY(reply_to) REFERENCES messages(id) ON DELETE SET NULL
            )
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn insert(
        &self,
        tx: &mut SqliteConnection,
        id: &str,
        conversation: &str,
        source: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> Result<(), Error> {
        let created = time_now();
        query(
            r#"
            INSERT INTO messages (id, conversation, source, text, created, reply_to)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(id)
        .bind(conversation)
        .bind(source)
        .bind(text)
        .bind(created)
        .bind(reply_to)
        .execute(tx)
        .await?;
        Ok(())
    }

    pub async fn is_sender(&self, id: &str, user: &str) -> Result<bool, Error> {
        let v: i64 = sqlx::query_scalar(
            r#"SELECT EXISTS(
                SELECT 1 FROM messages WHERE id = ? AND source = ?
            )"#,
        )
        .bind(id)
        .bind(user)
        .fetch_one(self.pool())
        .await?;

        Ok(v != 0)
    }
}

// --------------------
// Receipt API
// --------------------

impl ReceiptRepo {
    pub async fn create_table(&self) -> Result<(), Error> {
        query(
            r#"
            CREATE TABLE IF NOT EXISTS message_receipts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message TEXT NOT NULL,
                user TEXT NOT NULL,
                delivered_at INTEGER,
                read_at INTEGER,
                reaction INTEGER,
                UNIQUE(message, user),
                FOREIGN KEY(message) REFERENCES messages(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn upsert(
        &self,
        tx: &mut SqliteConnection,
        message: &str,
        user: &str,
        delivered: bool,
        read: bool,
        reaction: Option<i64>,
    ) -> Result<(), Error> {
        let now = time_now();
        let delivered_at = if delivered { Some(now) } else { None };
        let read_at = if read { Some(now) } else { None };

        query(
            r#"
            INSERT INTO message_receipts (message, user, delivered_at, read_at, reaction)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(message, user)
            DO UPDATE SET
                delivered_at = COALESCE(message_receipts.delivered_at, excluded.delivered_at),
                read_at      = COALESCE(message_receipts.read_at, excluded.read_at),
                reaction     = COALESCE(excluded.reaction, message_receipts.reaction)
            "#,
        )
        .bind(message)
        .bind(user)
        .bind(delivered_at)
        .bind(read_at)
        .bind(reaction)
        .execute(tx)
        .await?;

        Ok(())
    }
}
