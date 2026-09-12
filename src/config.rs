use std::env;

pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub binance_raw: String,
    pub binance_combined: String,
    pub discord_token: String,
    pub discord_guild_id: Option<u64>,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        dotenvy::dotenv().ok();

        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env");
        let redis_url = env::var("REDIS_URL").expect("REDIS_URL must be set in .env");
        let binance_raw = env::var("BINANCE_WEBSOCKET_RAW_ENDPOINT").expect("BINANCE_WEBSOCKET_RAW_ENDPOINT must be set in .env");
        let binance_combined = env::var("BINANCE_WEBSOCKET_STREAM_ENDPOINT").expect("BINANCE_WEBSOCKET_STREAM_ENDPOINT must be set in .env");
        let discord_token = env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN must be set in .env");
        let discord_guild_id = env::var("DISCORD_GUILD_ID").ok().and_then(|id| id.parse::<u64>().ok());

        Ok(Self {
            database_url,
            redis_url,
            binance_raw,
            binance_combined,
            discord_token,
            discord_guild_id,
        })
    }
}