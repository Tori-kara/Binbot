use std::env;

pub struct Config {
    pub database_url: String,
    pub redis_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        dotenvy::dotenv().ok();

        let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env");
        let redis_url = env::var("REDIS_URL").expect("REDIS_URL must be set in .env");

        Ok(Self {
            database_url,
            redis_url,
        })
    }
}