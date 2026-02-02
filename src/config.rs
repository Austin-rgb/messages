use crate::handlers::*;
use actix_web::web::{self, ServiceConfig};

pub fn config(cfg: &mut ServiceConfig) {
    cfg.service(
        web::scope("")
            .service(create_conversation)
            .service(get_conversation)
            .service(post_message)
            .service(peer_message)
            .service(get_messages)
            .service(get_pmessages)
            .service(react)
            .service(get_receipts)
            .service(mark_as_read)
            .service(list_conversations),
    );
}
