mod binance;
mod config;
mod error;
mod storage;

use binance::{ticker_stream, BinanceEvent, BinanceWebSocketClient, BinanceWsConfig};
use config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install default crypto provider for Rustls 0.23
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "Failed to install rustls crypto provider")?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Binbot starting...");

    let config = Config::from_env()?;
    tracing::info!(
        raw_endpoint = %config.binance_raw,
        combined_endpoint = %config.binance_combined,
        "Configuration loaded"
    );

    // 1. Test Database connection
    tracing::info!("Connecting to PostgreSQL database...");
    let db_pool = storage::db::init_db(&config.database_url).await?;
    sqlx::query("SELECT 1").execute(&db_pool).await?;
    tracing::info!("Database connection verified successfully (SELECT 1)");

    // Run pending migrations
    storage::db::run_migrations(&db_pool).await?;
    tracing::info!("Database migrations executed/verified successfully");

    // 2. Test Redis connection
    tracing::info!("Connecting to Redis...");
    let mut redis_conn = storage::redis::init_redis(&config.redis_url).await?;
    let pong: String = redis::cmd("PING").query_async(&mut redis_conn).await?;
    tracing::info!(response = %pong, "Redis connection verified successfully");

    // 3. Initialize Binance WebSocket client with configured raw stream endpoint (/ws)
    tracing::info!("Connecting to Binance WebSocket...");
    let ws_config = BinanceWsConfig::new(&config.binance_raw);
    let ws_client = BinanceWebSocketClient::connect(ws_config)?;

    // Listen to incoming market events
    let mut event_rx = ws_client.subscribe_events();

    // Subscribe to real-time ticker updates for major cryptocurrencies
    let test_streams = vec![
        ticker_stream("BTCUSDT"),
        ticker_stream("ETHUSDT"),
        ticker_stream("SOLUSDT"),
    ];
    tracing::info!(streams = ?test_streams, "Subscribing to Binance streams");
    ws_client.subscribe(test_streams).await?;

    // Spawn an event consumer loop that logs real-time price updates
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match event {
                BinanceEvent::Ticker(ticker) => {
                    tracing::info!(
                        symbol = %ticker.symbol,
                        price = %ticker.current_close,
                        change_pct = %ticker.price_change_percent,
                        high = %ticker.high_price,
                        low = %ticker.low_price,
                        volume = %ticker.total_base_volume,
                        "Market Ticker Update"
                    );
                }
                BinanceEvent::MiniTicker(mini) => {
                    tracing::info!(
                        symbol = %mini.symbol,
                        price = %mini.current_close,
                        "Mini Ticker Update"
                    );
                }
                BinanceEvent::Trade(trade) => {
                    tracing::info!(
                        symbol = %trade.symbol,
                        price = %trade.price,
                        quantity = %trade.quantity,
                        "Trade Execution"
                    );
                }
                BinanceEvent::AggTrade(agg) => {
                    tracing::info!(
                        symbol = %agg.symbol,
                        price = %agg.price,
                        quantity = %agg.quantity,
                        "Aggregate Trade"
                    );
                }
                BinanceEvent::Kline(kline) => {
                    tracing::info!(
                        symbol = %kline.symbol,
                        close = %kline.kline.close_price,
                        interval = %kline.kline.interval,
                        is_closed = %kline.kline.is_closed,
                        "Kline/Candle Update"
                    );
                }
            }
        }
    });

    // Keep running and handle graceful shutdown on Ctrl+C
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received, shutting down Binbot...");
    ws_client.shutdown().await?;

    Ok(())
}
