use redis::{AsyncCommands, Client};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::HashMap, fmt, sync::Arc};
use tokio::sync::RwLock;

/// Cache errors
#[derive(Debug)]
pub enum CacheError {
    Redis(redis::RedisError),
    Serde(serde_json::Error),
    Fallback,
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Redis(e) => write!(f, "Redis error: {}", e),
            CacheError::Serde(e) => write!(f, "Serde error: {}", e),
            CacheError::Fallback => write!(f, "Fallback error"),
        }
    }
}

impl From<redis::RedisError> for CacheError {
    fn from(e: redis::RedisError) -> Self {
        CacheError::Redis(e)
    }
}

impl From<serde_json::Error> for CacheError {
    fn from(e: serde_json::Error) -> Self {
        CacheError::Serde(e)
    }
}

/// Fully async generic cache: local + Redis
#[derive(Clone)]
pub struct Cache<T: Clone + Send + Sync + 'static> {
    local: Arc<RwLock<HashMap<String, T>>>,
    redis: Client,
    ttl_secs: usize,
}

impl<T> Cache<T>
where
    T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
{
    pub fn new(redis: Client, ttl_secs: usize) -> Self {
        Self {
            local: Arc::new(RwLock::new(HashMap::new())),
            redis,
            ttl_secs,
        }
    }

    /// Get value from cache, fallback to async DB function if not present
    pub async fn get<F, Fut>(&self, key: &str, db_fallback: F) -> Result<T, CacheError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, CacheError>>,
    {
        // 1️⃣ Check local cache
        if let Some(v) = self.local.read().await.get(key) {
            return Ok(v.clone());
        }

        // 2️⃣ Check Redis
        let mut con = self.redis.get_async_connection().await?;
        if let Ok(json) = con.get::<_, String>(key).await {
            let val: T = serde_json::from_str(&json)?;
            self.local
                .write()
                .await
                .insert(key.to_string(), val.clone());
            return Ok(val);
        }

        // 3️⃣ Fall back to DB
        let val = db_fallback().await?;
        self.set(key, val.clone()).await?;
        Ok(val)
    }

    /// Set value in both local and Redis caches
    pub async fn set(&self, key: &str, value: T) -> Result<(), CacheError> {
        self.local
            .write()
            .await
            .insert(key.to_string(), value.clone());
        let mut con = self.redis.get_async_connection().await?;
        let json = serde_json::to_string(&value)?;
        let _: () = con
            .set_ex(key, json, self.ttl_secs.try_into().unwrap())
            .await?;
        Ok(())
    }

    /// Remove value from both local and Redis
    pub async fn remove(&self, key: &str) -> Result<(), CacheError> {
        self.local.write().await.remove(key);
        let mut con = self.redis.get_async_connection().await?;
        let _: () = con.del(key).await?;
        Ok(())
    }

    /// Clear only local cache
    pub async fn clear_local(&self) {
        self.local.write().await.clear();
    }
}
