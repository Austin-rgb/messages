use std::process::exit;

use actix_web::{App, HttpServer};
use dotenvy::dotenv;
use messaging::MessagingModule;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let mm = match MessagingModule::new().await {
        Ok(r) => r,
        Err(r) => {
            eprintln!("Error in initializing messaging module: {}", r);
            exit(1)
        }
    };

    HttpServer::new(move || App::new().configure(|cfg| mm.config(cfg, "messages")))
        .workers(12)
        .bind(("127.0.0.1", 8082))?
        .run()
        .await
}
