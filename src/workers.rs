use crate::{
    models::{InsertMessage, Receipt},
    repositories::{Message, MessageReceipt},
};
use libworkers::BatchHandler;
use sqlx::SqlitePool;

pub struct IMHandler {}

impl BatchHandler<InsertMessage> for IMHandler {
    async fn handle(&self, msgs: Vec<(String, InsertMessage)>, db: SqlitePool) -> Vec<String> {
        // compute IDs before async move
        let (ack_ids, msgs): (Vec<String>, Vec<InsertMessage>) = msgs.into_iter().unzip();

        // insert messages (msgs is moved here)
        match Message::insert_many(&db, msgs).await {
            Ok(_) => ack_ids,
            Err(e) => {
                eprintln!("DB insert failed: {}", e);
                Vec::new()
            }
        }
    }
}

pub struct ReceiptHandler {}

impl BatchHandler<Receipt> for ReceiptHandler {
    async fn handle(&self, receipts: Vec<(String, Receipt)>, db: SqlitePool) -> Vec<String> {
        let (ids, receipts): (Vec<String>, Vec<Receipt>) = receipts.into_iter().unzip();
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
    }
}
