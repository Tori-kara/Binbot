use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub binance_raw: String,
    pub binance_combined: String,
    pub discord_token: String,
    pub discord_guild_id: Option<u64>,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        dotenvy::dotenv().ok();

        let database_url = env::var("DATABASE_URL")
            .map_err(|_| "DATABASE_URL is required (set in environment or .env)")?;
        let redis_url = env::var("REDIS_URL")
            .map_err(|_| "REDIS_URL is required (set in environment or .env)")?;
        let discord_token = env::var("DISCORD_TOKEN")
            .map_err(|_| "DISCORD_TOKEN is required (set in environment or .env)")?;

        let binance_raw = env::var("BINANCE_WEBSOCKET_RAW_ENDPOINT")
            .unwrap_or_else(|_| "wss://stream.binance.com:9443/ws".to_string());
        let binance_combined = env::var("BINANCE_WEBSOCKET_STREAM_ENDPOINT")
            .unwrap_or_else(|_| "wss://stream.binance.com:9443/stream".to_string());

        let discord_guild_id = env::var("DISCORD_GUILD_ID")
            .ok()
            .and_then(|id| id.parse::<u64>().ok());

        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(10000);

        Ok(Self {
            database_url,
            redis_url,
            binance_raw,
            binance_combined,
            discord_token,
            discord_guild_id,
            port,
        })
    }

    /// Provides a concise, sanitized summary of loaded settings.
    pub fn summary(&self) -> String {
        let guild_desc = self
            .discord_guild_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "global".to_string());
        format!("port={}, guild={}", self.port, guild_desc)
    }
}