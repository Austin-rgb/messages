use redis::{Client, RedisResult, Value, aio::Connection, cmd, from_redis_value};
use serde::de::DeserializeOwned;
use serde_json::from_str;
use sqlx::SqlitePool;
use tokio::time::{Duration, sleep};

/// Ensure a Redis consumer group exists, creating it if necessary
///
/// # Arguments
/// * `conn` - mutable Redis connection
/// * `stream` - Redis stream name
/// * `group` - Consumer group name
pub async fn ensure_group(conn: &mut Connection, stream: &str, group: &str) {
    let res: RedisResult<()> = cmd("XGROUP")
        .arg("CREATE")
        .arg(stream)
        .arg(group)
        .arg("0")
        .arg("MKSTREAM")
        .query_async(conn)
        .await;

    if let Err(err) = res {
        if !err.to_string().contains("BUSYGROUP") {
            panic!("Failed to create group '{}:{}': {}", stream, group, err);
        }
    }
}

/// Parse Redis stream entries into (ID, payload) pairs
fn parse_stream_entries(v: Value) -> Vec<(String, String)> {
    let mut out = Vec::new();

    if let Value::Bulk(streams) = v {
        for stream in streams {
            if let Value::Bulk(items) = stream {
                if let Value::Bulk(entries) = &items[1] {
                    for entry in entries {
                        if let Value::Bulk(e) = entry {
                            let id: String = from_redis_value(&e[0]).unwrap();
                            if let Value::Bulk(kv) = &e[1] {
                                let payload: String = from_redis_value(&kv[1]).unwrap();
                                out.push((id, payload));
                            }
                        }
                    }
                }
            }
        }
    }

    out
}

/// Generic Redis stream worker
///
/// # Arguments
/// * `redis` - Redis client
/// * `db` - SQLite connection pool
/// * `stream_name` - Redis stream name
/// * `group_name` - Consumer group name
/// * `process_fn` - async fn to process a batch of messages and return IDs to ACK
pub async fn stream_worker<T, F, Fut>(
    redis: &Client,
    db: SqlitePool,
    stream_name: &str,
    group_name: &str,
    mut process_fn: F,
) where
    T: DeserializeOwned + Send + 'static,
    F: FnMut(Vec<(String, T)>, SqlitePool) -> Fut + Send + 'static,
    Fut: Future<Output = Vec<String>> + Send + 'static,
{
    let mut con = redis.get_async_connection().await.unwrap();
    ensure_group(&mut con, stream_name, group_name).await;

    let consumer = format!("worker-{}", 1);

    loop {
        // 1️⃣ Drain pending messages first
        let mut entries = match cmd("XREADGROUP")
            .arg("GROUP")
            .arg(group_name)
            .arg(&consumer)
            .arg("COUNT")
            .arg(100)
            .arg("BLOCK")
            .arg(5000)
            .arg("STREAMS")
            .arg(stream_name)
            .arg("0")
            .query_async::<_, Value>(&mut con)
            .await
        {
            Ok(v) => parse_stream_entries(v),
            Err(e) => {
                eprintln!("Redis read error (pending): {}", e);
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        // 2️⃣ If no pending, read new messages
        if entries.is_empty() {
            entries = match cmd("XREADGROUP")
                .arg("GROUP")
                .arg(group_name)
                .arg(&consumer)
                .arg("COUNT")
                .arg(100)
                .arg("BLOCK")
                .arg(5000)
                .arg("STREAMS")
                .arg(stream_name)
                .arg(">")
                .query_async::<_, Value>(&mut con)
                .await
            {
                Ok(v) => parse_stream_entries(v),
                Err(e) => {
                    eprintln!("Redis read error (new): {}", e);
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
        }

        if entries.is_empty() {
            sleep(Duration::from_millis(500)).await;
            continue;
        }

        // Deserialize payloads
        let mut batch: Vec<(String, T)> = Vec::new();
        for (id, payload) in &entries {
            match from_str::<T>(&payload) {
                Ok(msg) => batch.push((id.to_string(), msg)),
                Err(e) => eprintln!("Malformed message skipped: {}", e),
            }
        }

        if batch.is_empty() {
            continue;
        }

        // Process batch and determine which IDs to ACK
        let ack_ids = process_fn(batch, db.clone()).await;

        if !ack_ids.is_empty() {
            let _: bool = match cmd("XACK")
                .arg(stream_name)
                .arg(group_name)
                .arg(ack_ids)
                .query_async::<Connection, i8>(&mut con)
                .await
            {
                Ok(_) => true,
                Err(e) => {
                    println!("redis ack failed: {}", e);
                    false
                }
            };
        }
        sleep(Duration::from_millis(500)).await;
    }
}
