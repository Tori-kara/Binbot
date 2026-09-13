use redis::aio::MultiplexedConnection;
use redis::Client;
use std::time::Duration;
use tokio::time::sleep;

use crate::market::MarketData;

/// Initializes the Redis multiplexed async connection with timeout and retry handling.
pub async fn init_redis(redis_url: &str) -> Result<MultiplexedConnection, redis::RedisError> {
    let client = Client::open(redis_url)?;

    let max_attempts = 4;
    let mut last_err = None;

    for attempt in 1..=max_attempts {
        tracing::info!("Connecting to Redis (attempt {attempt}/{max_attempts})...");

        match tokio::time::timeout(
            Duration::from_secs(8),
            client.get_multiplexed_async_connection(),
        )
        .await
        {
            Ok(Ok(conn)) => {
                return Ok(conn);
            }
            Ok(Err(err)) => {
                tracing::warn!("Redis connection attempt {attempt} failed: {err}");
                last_err = Some(err);
            }
            Err(_) => {
                tracing::warn!("Redis connection attempt {attempt} timed out after 8s");
                last_err = Some(
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Redis connection attempt timed out",
                    )
                    .into(),
                );
            }
        }

        if attempt < max_attempts {
            sleep(Duration::from_secs(attempt as u64)).await;
        }
    }

    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "Failed to connect to Redis after multiple attempts",
        )
        .into()
    }))
}

/// Helper store wrapping a Redis multiplexed connection for volatile market state
/// and distributed alert cooldown locks.
#[derive(Debug, Clone)]
pub struct RedisStore {
    conn: MultiplexedConnection,
}

impl RedisStore {
    pub fn new(conn: MultiplexedConnection) -> Self {
        Self { conn }
    }

    #[allow(dead_code)]
    pub fn connection(&self) -> MultiplexedConnection {
        self.conn.clone()
    }

    /// Stores a market snapshot for a symbol: `market:{SYMBOL}` with a TTL in seconds.
    pub async fn set_market_snapshot(
        &self,
        data: &MarketData,
        ttl_seconds: u64,
    ) -> Result<(), redis::RedisError> {
        let mut conn = self.conn.clone();
        let key = format!("market:{}", data.symbol.to_uppercase());
        let json = serde_json::to_string(data).map_err(|e| {
            let io_err = std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("MarketData serialization error: {e}"),
            );
            redis::RedisError::from(io_err)
        })?;

        if ttl_seconds > 0 {
            let _: () = redis::cmd("SET")
                .arg(&key)
                .arg(json)
                .arg("EX")
                .arg(ttl_seconds)
                .query_async(&mut conn)
                .await?;
        } else {
            let _: () = redis::cmd("SET")
                .arg(&key)
                .arg(json)
                .query_async(&mut conn)
                .await?;
        }

        Ok(())
    }

    /// Fetches the latest market snapshot for a symbol from Redis: `market:{SYMBOL}`
    pub async fn get_market_snapshot(
        &self,
        symbol: &str,
    ) -> Result<Option<MarketData>, redis::RedisError> {
        let mut conn = self.conn.clone();
        let key = format!("market:{}", symbol.to_uppercase());
        let json: Option<String> = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut conn)
            .await?;

        match json {
            Some(s) => {
                let data: MarketData = serde_json::from_str(&s).map_err(|e| {
                    let io_err = std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("MarketData deserialization error: {e}"),
                    );
                    redis::RedisError::from(io_err)
                })?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Atomically sets a cooldown lock: `SET cooldown:{alert_id} 1 EX {cooldown_seconds} NX`
    /// Returns `true` if the lock was acquired (not in cooldown), or `false` if already in cooldown.
    pub async fn try_set_cooldown(
        &self,
        alert_id: i64,
        cooldown_seconds: u32,
    ) -> Result<bool, redis::RedisError> {
        let mut conn = self.conn.clone();
        let key = format!("cooldown:{}", alert_id);

        let res: Option<String> = redis::cmd("SET")
            .arg(&key)
            .arg("1")
            .arg("EX")
            .arg(cooldown_seconds)
            .arg("NX")
            .query_async(&mut conn)
            .await?;

        Ok(res.as_deref() == Some("OK"))
    }

    /// Checks whether an alert cooldown key exists in Redis: `EXISTS cooldown:{alert_id}`
    pub async fn is_cooling_down(&self, alert_id: i64) -> Result<bool, redis::RedisError> {
        let mut conn = self.conn.clone();
        let key = format!("cooldown:{}", alert_id);
        let exists: bool = redis::cmd("EXISTS")
            .arg(&key)
            .query_async(&mut conn)
            .await?;
        Ok(exists)
    }

    /// Retrieves remaining cooldown time: `TTL cooldown:{alert_id}`
    pub async fn get_cooldown_remaining(
        &self,
        alert_id: i64,
    ) -> Result<Option<chrono::Duration>, redis::RedisError> {
        let mut conn = self.conn.clone();
        let key = format!("cooldown:{}", alert_id);
        let ttl: i64 = redis::cmd("TTL")
            .arg(&key)
            .query_async(&mut conn)
            .await?;

        if ttl > 0 {
            Ok(Some(chrono::Duration::seconds(ttl)))
        } else {
            Ok(None)
        }
    }

    /// Clears the cooldown key: `DEL cooldown:{alert_id}`
    pub async fn clear_cooldown(&self, alert_id: i64) -> Result<(), redis::RedisError> {
        let mut conn = self.conn.clone();
        let key = format!("cooldown:{}", alert_id);
        let _: () = redis::cmd("DEL")
            .arg(&key)
            .query_async(&mut conn)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_redis_market_and_cooldown_integration() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        dotenvy::dotenv().ok();
        let redis_url = match std::env::var("REDIS_URL") {
            Ok(url) if !url.is_empty() => url,
            _ => {
                println!("Skipping live Redis test: REDIS_URL not set");
                return;
            }
        };

        let conn = match init_redis(&redis_url).await {
            Ok(c) => c,
            Err(e) => {
                println!("Skipping live Redis test (connection failed: {e})");
                return;
            }
        };

        let store = RedisStore::new(conn);

        // 1. Test Market Snapshot
        let test_data = MarketData {
            symbol: "TESTCOIN".to_string(),
            price: dec!(1234.56),
            price_change_24hr: dec!(12.34),
            price_change_percent_24hr: dec!(1.01),
            high_price_24hr: dec!(1300.00),
            low_price_24hr: dec!(1200.00),
            volume_24hr: dec!(5000.0),
            quote_volume_24hr: dec!(6000000.0),
            update_at: Utc::now(),
        };

        store
            .set_market_snapshot(&test_data, 60)
            .await
            .expect("Set snapshot failed");
        let fetched = store
            .get_market_snapshot("TESTCOIN")
            .await
            .expect("Get snapshot failed");
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.symbol, "TESTCOIN");
        assert_eq!(fetched.price, dec!(1234.56));

        // 2. Test Cooldown Distributed Lock
        let test_alert_id = 99999999;
        // Ensure clean state
        let _ = store.clear_cooldown(test_alert_id).await;

        // First attempt: should acquire lock
        let acquired1 = store
            .try_set_cooldown(test_alert_id, 30)
            .await
            .expect("try_set_cooldown failed");
        assert!(acquired1, "Expected first cooldown acquisition to succeed");

        // Immediate second attempt: should fail (still cooling down)
        let acquired2 = store
            .try_set_cooldown(test_alert_id, 30)
            .await
            .expect("try_set_cooldown failed");
        assert!(!acquired2, "Expected second cooldown acquisition to be suppressed");

        // Verify is_cooling_down
        assert!(store.is_cooling_down(test_alert_id).await.unwrap());

        // Verify remaining TTL
        let rem = store.get_cooldown_remaining(test_alert_id).await.unwrap();
        assert!(rem.is_some());
        assert!(rem.unwrap().num_seconds() > 0);

        // Clear cooldown
        store.clear_cooldown(test_alert_id).await.unwrap();
        assert!(!store.is_cooling_down(test_alert_id).await.unwrap());
    }
}


