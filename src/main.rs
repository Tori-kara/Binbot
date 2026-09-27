mod alerts;
mod binance;
mod config;
mod currency;
mod discord;
mod error;
mod market;
mod storage;
mod web;

use std::sync::Arc;
use std::time::Duration;

#[allow(unused_imports)]
use binance::{all_market_tickers_stream, ticker_stream, BinanceWebSocketClient, BinanceWsConfig};
use config::Config;
use currency::CurrencyService;
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

    let config = match Config::from_env() {
        Ok(cfg) => cfg,
        Err(err) => {
            tracing::error!("Config error: {err}");
            return Err(err);
        }
    };
    tracing::info!("✓ Config loaded ({})", config.summary());

    // 0. Render Web Service Health Check & Bot Invitation Server
    web::start_health_server(config.port, config.discord_bot_url.clone()).await;

    // 1. PostgreSQL database connection & migrations
    let db_pool = storage::db::init_db(&config.database_url).await?;
    sqlx::query("SELECT 1").execute(&db_pool).await?;
    storage::db::run_migrations(&db_pool).await?;
    tracing::info!("✓ PostgreSQL connected & migrations up to date");

    // 2. Redis connection
    let (redis_conn_opt, redis_store_opt) = match storage::redis::init_redis(&config.redis_url).await {
        Ok(mut conn) => {
            let ping_res = tokio::time::timeout(
                Duration::from_secs(5),
                redis::cmd("PING").query_async::<String>(&mut conn),
            )
            .await;

            match ping_res {
                Ok(Ok(_pong)) => {
                    tracing::info!("✓ Redis connected & verified");
                    let store = storage::redis::RedisStore::new(conn.clone());
                    (Some(conn), Some(store))
                }
                Ok(Err(err)) => {
                    tracing::warn!("Redis PING failed: {err}. Operating in high-performance in-memory fallback mode.");
                    (None, None)
                }
                Err(_) => {
                    tracing::warn!("Redis PING timed out after 5s. Operating in high-performance in-memory fallback mode.");
                    (None, None)
                }
            }
        }
        Err(err) => {
            tracing::warn!("Redis initialization failed: {err}. Operating in high-performance in-memory fallback mode.");
            (None, None)
        }
    };

    // 3. Binance WebSocket connection
    let ws_config = BinanceWsConfig::new(&config.binance_raw);
    let ws_client = BinanceWebSocketClient::connect(ws_config)?;

    // Listen to incoming market events
    let event_rx = ws_client.subscribe_events();

    // Subscribe to real-time ticker updates (simply add or remove coins in this list)
    let streams = vec![
        ticker_stream("BTCUSDT"),
        ticker_stream("ETHUSDT"),
        ticker_stream("SOLUSDT"),
        ticker_stream("BNBUSDT"),
        ticker_stream("XRPUSDT"),
        ticker_stream("DOGEUSDT"),
        ticker_stream("ADAUSDT"),
        ticker_stream("AVAXUSDT"),
        ticker_stream("SUIUSDT"),
        ticker_stream("PEPEUSDT"),
        ticker_stream("SHIBUSDT"),
        ticker_stream("LINKUSDT"),
        ticker_stream("NEARUSDT"),
        ticker_stream("DOTUSDT"),
        ticker_stream("LTCUSDT"),
        ticker_stream("BCHUSDT"),
        ticker_stream("UNIUSDT"),
        ticker_stream("APTUSDT"),
        ticker_stream("RENDERUSDT"),
        ticker_stream("FETUSDT"),
        ticker_stream("TAOUSDT"),
        ticker_stream("INJUSDT"),
        ticker_stream("WIFUSDT"),
        ticker_stream("BONKUSDT"),
        ticker_stream("FLOKIUSDT"),
        ticker_stream("TIAUSDT"),
        ticker_stream("SEIUSDT"),
        ticker_stream("ARBUSDT"),
        ticker_stream("OPUSDT"),
        ticker_stream("TRXUSDT"),
        // Streams real-time 24h rolling stats for all additional Binance market pairs
        all_market_tickers_stream(),
    ];
    ws_client.subscribe(streams).await?;
    tracing::info!("✓ Binance WebSocket connected (subscribing to market streams)");

    // 4. Currency Conversion Engine
    let check_interval_hours = std::env::var("CURRENCY_CHECK_INTERVAL_HOURS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(3);
    let check_interval = Duration::from_secs(check_interval_hours * 3600);

    let currency_service = Arc::new(CurrencyService::new(
        redis_conn_opt.clone(),
        rust_decimal_macros::dec!(0.1),
    ));
    currency_service.init_cache().await;
    currency_service
        .clone()
        .spawn_interval_checker(check_interval);
    tracing::info!(
        "✓ Fiat Currency Engine initialized (interval check: {}h, Redis active: {})",
        check_interval_hours,
        redis_conn_opt.is_some()
    );

    // 5. Market Data Engine
    let market_state = match &redis_store_opt {
        Some(store) => MarketState::with_redis(store.clone()),
        None => MarketState::new(),
    };
    let (update_tx, _update_rx) = tokio::sync::broadcast::channel(16384);
    let alert_market_rx = update_tx.subscribe();
    let mut processor = MarketProcessor::new(market_state.clone(), update_tx);
    if let Some(store) = &redis_store_opt {
        processor = processor
            .with_redis(store.clone())
            .with_redis_cache_config(
                config.redis_market_cache_enabled,
                config.redis_market_cache_interval_secs,
            );
    }
    tracing::info!(
        "✓ Market Data Engine initialized (Redis cache: enabled={}, interval={}s)",
        config.redis_market_cache_enabled,
        config.redis_market_cache_interval_secs
    );

    // Spawn background market data processor
    tokio::spawn(async move {
        processor.run(event_rx).await;
    });

    // 6. Alert Engine & Store
    let mut alert_store_builder = alerts::AlertStore::new(db_pool.clone());
    if let Some(store) = &redis_store_opt {
        alert_store_builder = alert_store_builder.with_redis(store.clone());
    }
    let alert_store = Arc::new(alert_store_builder);
    match alert_store.load_all_active_alerts().await {
        Ok(count) => {
            tracing::info!("✓ Hydrated {count} active alert rules from PostgreSQL");
        }
        Err(e) => {
            tracing::warn!("Failed to load active alerts from database: {e}");
        }
    }
    let (alert_notif_tx, alert_notif_rx) = tokio::sync::mpsc::channel(1000);
    let alert_engine = alerts::AlertEngine::new(alert_store.clone(), alert_notif_tx)
        .with_market_state(market_state.clone());

    tokio::spawn(async move {
        alert_engine.run(alert_market_rx).await;
    });
    tracing::info!("✓ Alert Engine & Distributed Cooldown Processor initialized");

    // 7. Discord Bot Integration
    let discord_token = config.discord_token.clone();
    let discord_guild_id = config.discord_guild_id;
    let bot_market_state = market_state.clone();
    let bot_currency_service = currency_service.clone();
    let bot_alert_store = alert_store.clone();

    let _bot_handle = tokio::spawn(async move {
        if let Err(e) = discord::run_bot(
            discord_token,
            bot_market_state,
            bot_currency_service,
            bot_alert_store,
            alert_notif_rx,
            discord_guild_id,
        )
        .await
        {
            tracing::error!("Discord bot stopped with error: {:?}", e);
        }
    });
    tracing::info!("✓ Discord Bot service spawned");

    // Keep running and handle graceful shutdown on Ctrl+C
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received, shutting down Binbot...");
    ws_client.shutdown().await?;
    tracing::info!("Binbot shutdown complete.");

    Ok(())
}
