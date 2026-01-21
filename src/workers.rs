use crate::{
    libworkers::stream_worker,
    models::{InsertMessage, Receipt},
    repositories::{Message, MessageReceipt},
};
use redis::Client;
use sqlx::SqlitePool;

/// DB worker: insert messages from Redis stream safely
pub async fn db_worker(redis: &Client, db: &SqlitePool) {
    // clone the pool so async block owns it
    let db = db.clone();

    stream_worker::<InsertMessage, _, _>(
        redis,
        db,
        "messages_stream",
        "db_group",
        async move |msgs: Vec<(String, InsertMessage)>, db: SqlitePool| {
            // compute IDs before async move
            let (ack_ids, msgs): (Vec<String>, Vec<InsertMessage>) = msgs.into_iter().unzip();
            //async move {
            // insert messages (msgs is moved here)
            match Message::insert_many(&db, msgs).await {
                Ok(_) => {
                    println!("inserted {} msgs", ack_ids.len());
                    ack_ids
                }
                Err(e) => {
                    eprintln!("DB insert failed: {}", e);
                    Vec::new()
                }
            }
            //}
        },
    )
    .await;
}

/// Receipt worker: insert message receipts safely
pub async fn receipt_worker(redis: &Client, db: &SqlitePool) {
    stream_worker::<Receipt, _, _>(
        redis,
        db.clone(),
        "receipts_stream",
        "receipts_group",
        async move |receipts, db| {
            let (ids, receipts): (Vec<String>, Vec<Receipt>) = receipts.into_iter().unzip();
            //async move {
            let mut tx = match db.begin().await {
                Ok(tx) => tx,
                Err(e) => {
                    eprintln!("DB transaction start failed: {}", e);
                    return Vec::new(); // return empty vec
                }
            };

            let _ = MessageReceipt::batch_upsert(&mut tx, &receipts).await;
            let _ = tx.commit().await;
            ids
        }, //},
    )
    .await;
}
