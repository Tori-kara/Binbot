use redis::aio::MultiplexedConnection;
use redis::Client;
use std::time::Duration;
use tokio::time::sleep;

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

