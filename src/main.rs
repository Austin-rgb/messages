use actix_web::{web, App, HttpServer, HttpResponse, get, post, Responder,web::{scope,Path}};
use sqlx::{SqlitePool, FromRow, query, query_as};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use rand::Rng;

use auth_middleware::{middleware::Auth,common::Claims};

struct AppState {
    db: SqlitePool,
}

#[derive(Serialize, sqlx::FromRow)]
struct MessageResponse {
    id: i64,
    conversation: String,
    source: String,
    text: String,
    created: i64,
}

#[derive(Deserialize)]
struct CreateMessage {
    text: String,     // message content
}
#[derive(Serialize, FromRow)]
struct ConversationListItem {
    name: String,
    title: Option<String>,
    admin: String,
    created: i64,
}

#[derive(Deserialize)]
struct CreateConversation {
    participants: Vec<String>,
    name:Option<String>,
    title:Option<String>
}
#[derive(Serialize, sqlx::FromRow)]
struct ConversationResponse {
    name:String,
    admin: String,
    title:Option<String>,
    created: i64,
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
struct Participant {
    id: i64,
    conversation: String,
    participant: String,
    created: i64,
}


fn random_string(len: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();

    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

async fn init_db(db: &SqlitePool) -> Result<(), sqlx::Error> {
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
    .await?;

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
    .await?;

    query(
        r#"
        CREATE TABLE IF NOT EXISTS deliveries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            message INTEGER NOT NULL,
            dest TEXT NOT NULL
        )
        "#,
    )
    .execute(db)
    .await?;

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
    .await?;
    
    // Add indexes
    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_messages_conversation 
        ON messages(conversation, created)
        "#
    )
    .execute(db)
    .await?;
    
    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_participants_conversation 
        ON participants(conversation)
        "#
    )
    .execute(db)
    .await?;
    
    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_participants_user 
        ON participants(participant)
        "#
    )
    .execute(db)
    .await?;
  
    Ok(())
}



#[post("/conversations")]
async fn create_conversation(
    state: web::Data<AppState>,
claims:web::ReqData<Claims>,
    payload: web::Json<CreateConversation>,
) -> impl Responder {
    // Validation
    if payload.participants.is_empty() {
        return HttpResponse::BadRequest()
            .body("Conversation must have at least one participant");
    }

    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    // 1️⃣ Create conversation (admin = creator)
    let conversation = match query_as::<_, ConversationResponse>(
        r#"
        INSERT INTO conversations (name, title, admin, created)
        VALUES (?, ?, ?, ?)
        RETURNING title, name, admin, created
        "#,
    )
    .bind(&payload.name.clone().unwrap_or_else(|| random_string(16)))
        .bind(&payload.title)
    .bind(&claims.sub)
    .bind(created)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Conversation insert error: {:?}", e);
            return HttpResponse::InternalServerError().finish();
        }
    };

    // 2️⃣ Insert creator as participant
    if let Err(e) = query(
        r#"
        INSERT INTO participants (conversation, participant, created)
        VALUES (?, ?, ?)
        "#,
    )
    .bind(conversation.name.clone())
    .bind(&claims.sub)
    .bind(created)
    .execute(&mut *tx)
    .await
    {
        eprintln!("Participant insert error: {:?}", e);
        return HttpResponse::InternalServerError().finish();
    }

    // 3️⃣ Insert other participants (deduplicated)
    for user in payload.participants.iter().filter(|u| *u != &claims.sub) {
        if let Err(e) = query(
            r#"
            INSERT INTO participants (conversation, participant, created)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(&conversation.name)
        .bind(user)
        .bind(created)
        .execute(&mut *tx)
        .await
        {
            eprintln!("Participant insert error: {:?}", e);
            return HttpResponse::InternalServerError().finish();
        }
    }

    // 4️⃣ Commit transaction
    if tx.commit().await.is_err() {
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Ok().json(conversation)
}

#[derive(Deserialize)]
struct ConversationQuery {
    user: String,
}

#[get("/conversations")]
async fn list_conversations(
    state: web::Data<AppState>,
    claims:web::ReqData<Claims>
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
    .bind(&claims.sub)
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
    claims: web::ReqData<Claims>,
    path: Path<String>,
) -> impl Responder {
    let conversation_name = path.into_inner();
    
    // Check if user is a participant
    let is_participant: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
            SELECT 1 FROM participants 
            WHERE conversation = ? AND participant = ?
        )"#
    )
    .bind(&conversation_name)
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .unwrap_or(false);
    
    if !is_participant {
        return HttpResponse::Forbidden()
            .body("Not a participant in this conversation");
    }
    
    let conversation = query_as::<_, ConversationResponse>(
        r#"
        SELECT name, title, admin, created
        FROM conversations
        WHERE name = ?
        "#
    )
    .bind(&conversation_name)
    .fetch_one(&state.db)
    .await;
    
    match conversation {
        Ok(conv) => HttpResponse::Ok().json(conv),
        Err(sqlx::Error::RowNotFound) => HttpResponse::NotFound()
            .body("Conversation not found"),
        Err(e) => {
            eprintln!("Error fetching conversation: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[post("/conversations/{name}/messages")]
async fn post_message(
    state: web::Data<AppState>,
claims:web::ReqData<Claims>,
    path: Path<String>,
    payload: web::Json<CreateMessage>,
) -> impl Responder {
    let conversation_name = path.into_inner();

    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // validate that participation and conversation exists
    
let allowed = sqlx::query_scalar::<_, i64>(
    r#"
    SELECT COUNT(*)
    FROM conversations c
    JOIN participants p
      ON p.conversation = c.name
    WHERE c.name = ?
      AND p.participant = ?
    "#
)
.bind(&conversation_name)
.bind(&claims.sub)
.fetch_one(&state.db)
.await;

match allowed {
    Ok(count) if count > 0 => {}
    Ok(_) => return HttpResponse::Forbidden()
        .body("Not a participant in this conversation"),
    Err(_) => return HttpResponse::InternalServerError().finish(),
}
    

    // Insert message
    let result = sqlx::query_as::<_, MessageResponse>(
        r#"
        INSERT INTO messages ( conversation, source, text, created)
        VALUES ( ?, ?, ?, ?)
        RETURNING id, conversation, source, text, created
        "#
    )
    .bind(&conversation_name)
    .bind(&claims.sub)
    .bind(&payload.text)
    .bind(created)
    .fetch_one(&state.db)
    .await;

    match result {
        Ok(message) => HttpResponse::Ok().json(message),
        Err(e) => {
            eprintln!("Insert message error: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[derive(Deserialize)]
struct GetMessagesQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    before: Option<i64>,
}

fn default_limit() -> i64 {
    50
}

#[get("/conversations/{name}/messages")]
async fn get_messages(
    state: web::Data<AppState>,
    claims: web::ReqData<Claims>,
    path: Path<String>,
    query: web::Query<GetMessagesQuery>,
) -> impl Responder {
    let conversation_name = path.into_inner();
    
    // Check if user is a participant
    let is_participant: bool = sqlx::query_scalar(
        r#"SELECT EXISTS(
            SELECT 1 FROM participants 
            WHERE conversation = ? AND participant = ?
        )"#
    )
    .bind(&conversation_name)
    .bind(&claims.sub)
    .fetch_one(&state.db)
    .await
    .unwrap_or(false);
    
    if !is_participant {
        return HttpResponse::Forbidden()
            .body("Not a participant in this conversation");
    }
    
    let messages = match query.before {
        Some(before_id) => {
            query_as::<_, MessageResponse>(
                r#"
                SELECT id, conversation, source, text, created
                FROM messages
                WHERE conversation = ? AND id < ?
                ORDER BY created DESC
                LIMIT ?
                "#
            )
            .bind(&conversation_name)
            .bind(before_id)
            .bind(query.limit)
            .fetch_all(&state.db)
            .await
        }
        None => {
            query_as::<_, MessageResponse>(
                r#"
                SELECT id, conversation, source, text, created
                FROM messages
                WHERE conversation = ?
                ORDER BY created DESC
                LIMIT ?
                "#
            )
            .bind(&conversation_name)
            .bind(query.limit)
            .fetch_all(&state.db)
            .await
        }
    };
    
    match messages {
        Ok(messages) => HttpResponse::Ok().json(messages),
        Err(e) => {
            eprintln!("Error fetching messages: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let db = SqlitePool::connect("sqlite://messages.db")
        .await
        .expect("Failed to connect to DB");

    init_db(&db).await.expect("DB init failed");

    let state = web::Data::new(AppState { db });

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
        .service(scope("")
        .wrap(Auth)
        .service(create_conversation)
        .service(get_conversation)
        .service(post_message)
        .service(get_messages)
        .service(list_conversations)
        )
        
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
