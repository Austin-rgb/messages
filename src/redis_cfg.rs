use std::env;

use redis::{Client, RedisResult, aio::Connection, cmd};

pub fn create_redis() -> Client {
    let redis_url = env::var("REDIS").unwrap();
    Client::open(redis_url).unwrap()
}

pub async fn ensure_receipts_group(conn: &mut redis::aio::Connection) {
    let res: redis::RedisResult<()> = redis::cmd("XGROUP")
        .arg("CREATE")
        .arg("receipts_stream")
        .arg("receipts_group")
        .arg("0")
        .arg("MKSTREAM")
        .query_async(conn)
        .await;

    if let Err(e) = res {
        if !e.to_string().contains("BUSYGROUP") {
            panic!("Failed to create receipts group: {}", e);
        }
    }
}

pub async fn ensure_group(conn: &mut Connection) {
    let res: RedisResult<()> = cmd("XGROUP")
        .arg("CREATE")
        .arg("messages_stream")
        .arg("db_group")
        .arg("0")
        .arg("MKSTREAM")
        .query_async(conn)
        .await;

    if let Err(err) = res {
        // BUSYGROUP means the group already exists — safe to ignore
        if !err.to_string().contains("BUSYGROUP") {
            panic!("Failed to create group: {err}");
        }
    }
}
