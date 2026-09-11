use sqlx::postgres::PgPoolOptions;
use sqlx::{Error, PgPool};

pub type DbPool = PgPool;

/// Initializes the PostgreSQL connection pool using the provided database URL.
pub async fn init_db(database_url: &str) -> Result<PgPool, Error> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;

    Ok(pool)
}

/// Runs embedded database migrations located in the `migrations` directory.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}