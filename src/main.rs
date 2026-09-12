mod binance;
mod config;
mod error;
mod market;
mod storage;

use binance::{ticker_stream, BinanceWebSocketClient, BinanceWsConfig};
use config::Config;
use market::{MarketProcessor, MarketState};

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
        .with_target(false)
        .init();

    tracing::info!("Initializing Binbot...");

    let config = Config::from_env()?;
    tracing::info!("✓ Configuration loaded");

    // 1. PostgreSQL database connection & migrations
    let db_pool = storage::db::init_db(&config.database_url).await?;
    sqlx::query("SELECT 1").execute(&db_pool).await?;
    storage::db::run_migrations(&db_pool).await?;
    tracing::info!("✓ PostgreSQL connected & migrations up to date");

    // 2. Redis connection
    let mut redis_conn = storage::redis::init_redis(&config.redis_url).await?;
    let _pong: String = redis::cmd("PING").query_async(&mut redis_conn).await?;
    tracing::info!("✓ Redis connected & verified");

    // 3. Binance WebSocket connection
    let ws_config = BinanceWsConfig::new(&config.binance_raw);
    let ws_client = BinanceWebSocketClient::connect(ws_config)?;

    // Listen to incoming market events
    let event_rx = ws_client.subscribe_events();

    // Subscribe to real-time ticker updates for major cryptocurrencies
    let test_streams = vec![
        ticker_stream("BTCUSDT"),
        ticker_stream("ETHUSDT"),
        ticker_stream("SOLUSDT"),
    ];
    ws_client.subscribe(test_streams).await?;
    tracing::info!("✓ Binance WebSocket connected (streams: BTC, ETH, SOL)");

    // 4. Market Data Engine
    let market_state = MarketState::new();
    let (update_tx, _update_rx) = tokio::sync::broadcast::channel(100);
    let processor = MarketProcessor::new(market_state.clone(), update_tx);
    tracing::info!("✓ Market Data Engine initialized");

    // Spawn background market data processor
    tokio::spawn(async move {
        processor.run(event_rx).await;
    });

    // Keep running and handle graceful shutdown on Ctrl+C
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received, shutting down Binbot...");
    ws_client.shutdown().await?;
    tracing::info!("Binbot shutdown complete.");

    Ok(())
}
