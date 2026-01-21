use std::env;

use redis::Client;

pub fn create_redis() -> Client {
    let redis_url = env::var("REDIS").unwrap();
    Client::open(redis_url).unwrap()
}
