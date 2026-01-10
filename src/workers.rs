use crate::{
    models::{InsertMessage, Receipt},
    redis_cfg::{ensure_group, ensure_receipts_group},
    repositories::{Message, MessageReceipt},
};
use redis::{Client, Value, cmd, from_redis_value};
use serde_json::from_str;
use sqlx::SqlitePool;

pub async fn receipt_worker(redis: &Client, db: &SqlitePool) {
    let mut conn = redis.get_async_connection().await.unwrap();
    ensure_receipts_group(&mut conn).await;

    loop {
        let res: Value = cmd("XREADGROUP")
            .arg("GROUP")
            .arg("receipts_group")
            .arg("worker-1")
            .arg("COUNT")
            .arg(200)
            .arg("BLOCK")
            .arg(5000)
            .arg("STREAMS")
            .arg("receipts_stream")
            .arg(">")
            .query_async(&mut conn)
            .await
            .unwrap();

        let entries = parse_receipt_stream(res);
        if entries.is_empty() {
            continue;
        }

        let mut tx = match db.begin().await {
            Ok(tx) => tx,
            Err(_) => continue,
        };

        for (_, payload) in &entries {
            let evt: Receipt = serde_json::from_str(payload).unwrap();

            // IMPORTANT: idempotent write
            let _ = MessageReceipt::upsert(
                &mut tx,
                evt.message_id.clone(),
                &evt.user_id,
                evt.delivered,
                evt.read,
                evt.reaction,
            )
            .await;
        }

        let _ = tx.commit().await;
    }
}

pub async fn db_worker(redis: &Client, db: &SqlitePool) {
    let mut con = redis.get_async_connection().await.unwrap();
    ensure_group(&mut con).await;

    loop {
        let streams: Value = cmd("XREADGROUP")
            .arg("GROUP")
            .arg("db_group")
            .arg("worker-1")
            .arg("COUNT")
            .arg(100)
            .arg("BLOCK")
            .arg(5000)
            .arg("STREAMS")
            .arg("messages_stream")
            .arg(">")
            .query_async(&mut con)
            .await
            .unwrap();
        let entries = parse_stream(streams);
        let len = entries.len();
        if len == 0 {
            continue;
        }
        let msgs: Vec<InsertMessage> = entries
            .iter()
            .map(|(a, x)| from_str::<InsertMessage>(x).unwrap())
            .collect();
        match Message::insert_many(db, msgs).await {
            Ok(_) => println!("inserted {} messages", len),
            Err(e) => println!("insertion failed: {} \n entries: {:?}", e, entries),
        };
    }
}

fn parse_receipt_stream(v: Value) -> Vec<(String, String)> {
    let mut out = Vec::new();

    if let Value::Bulk(streams) = v {
        for stream in streams {
            if let Value::Bulk(items) = stream {
                if let Value::Bulk(entries) = &items[1] {
                    for entry in entries {
                        if let Value::Bulk(e) = entry {
                            let id: String = redis::from_redis_value(&e[0]).unwrap();
                            if let Value::Bulk(kv) = &e[1] {
                                let payload: String = redis::from_redis_value(&kv[1]).unwrap();
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

fn parse_stream(v: Value) -> Vec<(String, String)> {
    let mut out = Vec::new();

    if let Value::Bulk(streams) = v {
        for stream in streams {
            if let Value::Bulk(items) = stream {
                let entries = &items[1];
                if let Value::Bulk(entries) = entries {
                    for entry in entries {
                        if let Value::Bulk(e) = entry {
                            let id: String = from_redis_value(&e[0]).unwrap();
                            let data = &e[1];
                            if let Value::Bulk(kv) = data {
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
