use redis::aio::MultiplexedConnection;
use redis::Client;

pub async fn init_redis(redis_url: &str) -> Result<MultiplexedConnection, redis::RedisError> {
    let client = Client::open(redis_url)?;
    let conn = client.get_multiplexed_async_connection().await?;
    Ok(conn)
}