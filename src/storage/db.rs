use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Error, PgPool, Row};

#[allow(dead_code)]
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

// ---------------------------------------------------------------------------
// Database Record DTOs
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GuildRecord {
    pub id: i64,
    pub guild_id: String,
    pub name: String,
    pub created_at: NaiveDateTime,
}

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChannelRecord {
    pub id: i64,
    pub guild_id: Option<i64>,
    pub channel_id: String,
    pub name: String,
    pub created_at: NaiveDateTime,
}

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRecord {
    pub id: i64,
    pub discord_id: String,
    pub username: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AlertRecord {
    pub id: i64,
    pub user_id: i64,
    pub channel_id: i64,
    pub symbol: String,
    pub condition_type: String,
    pub threshold: Decimal,
    pub baseline_price: Option<Decimal>,
    pub cooldown_seconds: i32,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub is_triggered: bool,
    pub enabled: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AlertWithDetails {
    pub id: i64,
    pub user_id: i64,
    pub user_discord_id: String,
    pub channel_id: i64,
    pub channel_discord_id: String,
    pub symbol: String,
    pub condition_type: String,
    pub threshold: Decimal,
    pub baseline_price: Option<Decimal>,
    pub cooldown_seconds: i32,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub is_triggered: bool,
    pub enabled: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

// ---------------------------------------------------------------------------
// Repositories
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GuildRepository<'a>(pub &'a PgPool);

impl<'a> GuildRepository<'a> {
    /// Inserts or updates a Discord guild by its discord ID and returns the primary key ID.
    pub async fn upsert(&self, guild_id: &str, name: &str) -> Result<i64, Error> {
        let row = sqlx::query(
            r#"
            INSERT INTO guilds (guild_id, name)
            VALUES ($1, $2)
            ON CONFLICT (guild_id) DO UPDATE SET name = EXCLUDED.name
            RETURNING id
            "#,
        )
        .bind(guild_id)
        .bind(name)
        .fetch_one(self.0)
        .await?;

        Ok(row.get("id"))
    }
}

#[derive(Debug, Clone)]
pub struct ChannelRepository<'a>(pub &'a PgPool);

impl<'a> ChannelRepository<'a> {
    /// Inserts or updates a Discord channel and returns its primary key ID.
    pub async fn upsert(
        &self,
        guild_id: Option<i64>,
        channel_id: &str,
        name: &str,
    ) -> Result<i64, Error> {
        let row = sqlx::query(
            r#"
            INSERT INTO channels (guild_id, channel_id, name)
            VALUES ($1, $2, $3)
            ON CONFLICT (channel_id) DO UPDATE SET
                name = EXCLUDED.name,
                guild_id = COALESCE(EXCLUDED.guild_id, channels.guild_id)
            RETURNING id
            "#,
        )
        .bind(guild_id)
        .bind(channel_id)
        .bind(name)
        .fetch_one(self.0)
        .await?;

        Ok(row.get("id"))
    }
}

#[derive(Debug, Clone)]
pub struct UserRepository<'a>(pub &'a PgPool);

impl<'a> UserRepository<'a> {
    /// Inserts or updates a Discord user and returns its primary key ID.
    pub async fn upsert(&self, discord_id: &str, username: &str) -> Result<i64, Error> {
        let row = sqlx::query(
            r#"
            INSERT INTO users (discord_id, username)
            VALUES ($1, $2)
            ON CONFLICT (discord_id) DO UPDATE SET username = EXCLUDED.username
            RETURNING id
            "#,
        )
        .bind(discord_id)
        .bind(username)
        .fetch_one(self.0)
        .await?;

        Ok(row.get("id"))
    }
}

#[derive(Debug, Clone)]
pub struct AlertRepository<'a>(pub &'a PgPool);

impl<'a> AlertRepository<'a> {
    /// Inserts a new alert rule into the database.
    pub async fn create(
        &self,
        user_id: i64,
        channel_id: i64,
        symbol: &str,
        condition_type: &str,
        threshold: Decimal,
        baseline_price: Option<Decimal>,
        cooldown_seconds: i32,
    ) -> Result<AlertRecord, Error> {
        let row = sqlx::query_as::<_, AlertRecord>(
            r#"
            INSERT INTO alerts (
                user_id,
                channel_id,
                symbol,
                condition_type,
                threshold,
                baseline_price,
                cooldown_seconds,
                is_triggered,
                enabled
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, FALSE, TRUE)
            RETURNING
                id,
                user_id,
                channel_id,
                symbol,
                condition_type,
                threshold,
                baseline_price,
                cooldown_seconds,
                last_triggered_at,
                is_triggered,
                enabled,
                created_at,
                updated_at
            "#,
        )
        .bind(user_id)
        .bind(channel_id)
        .bind(symbol)
        .bind(condition_type)
        .bind(threshold)
        .bind(baseline_price)
        .bind(cooldown_seconds)
        .fetch_one(self.0)
        .await?;

        Ok(row)
    }

    /// Fetches all active (enabled) alerts joined with user and channel discord IDs.
    pub async fn get_active_alerts(&self) -> Result<Vec<AlertWithDetails>, Error> {
        sqlx::query_as::<_, AlertWithDetails>(
            r#"
            SELECT 
                a.id,
                a.user_id,
                u.discord_id as user_discord_id,
                a.channel_id,
                c.channel_id as channel_discord_id,
                a.symbol,
                a.condition_type,
                a.threshold,
                a.baseline_price,
                a.cooldown_seconds,
                a.last_triggered_at,
                a.is_triggered,
                a.enabled,
                a.created_at,
                a.updated_at
            FROM alerts a
            JOIN users u ON a.user_id = u.id
            JOIN channels c ON a.channel_id = c.id
            WHERE a.enabled = TRUE
            ORDER BY a.id ASC
            "#,
        )
        .fetch_all(self.0)
        .await
    }

    /// Fetches all active alerts belonging to a specific Discord user.
    #[allow(dead_code)]
    pub async fn get_user_alerts(&self, user_discord_id: &str) -> Result<Vec<AlertWithDetails>, Error> {
        sqlx::query_as::<_, AlertWithDetails>(
            r#"
            SELECT 
                a.id,
                a.user_id,
                u.discord_id as user_discord_id,
                a.channel_id,
                c.channel_id as channel_discord_id,
                a.symbol,
                a.condition_type,
                a.threshold,
                a.baseline_price,
                a.cooldown_seconds,
                a.last_triggered_at,
                a.is_triggered,
                a.enabled,
                a.created_at,
                a.updated_at
            FROM alerts a
            JOIN users u ON a.user_id = u.id
            JOIN channels c ON a.channel_id = c.id
            WHERE u.discord_id = $1 AND a.enabled = TRUE
            ORDER BY a.id ASC
            "#,
        )
        .bind(user_discord_id)
        .fetch_all(self.0)
        .await
    }

    /// Deletes an alert by ID, verifying that it belongs to the given user Discord ID.
    pub async fn delete(&self, alert_id: i64, user_discord_id: &str) -> Result<bool, Error> {
        let res = sqlx::query(
            r#"
            DELETE FROM alerts
            WHERE id = $1 AND user_id = (SELECT id FROM users WHERE discord_id = $2)
            "#,
        )
        .bind(alert_id)
        .bind(user_discord_id)
        .execute(self.0)
        .await?;

        Ok(res.rows_affected() > 0)
    }

    /// Updates the trigger state and last triggered timestamp of an alert.
    pub async fn update_trigger_state(
        &self,
        alert_id: i64,
        is_triggered: bool,
        last_triggered_at: Option<NaiveDateTime>,
    ) -> Result<(), Error> {
        sqlx::query(
            r#"
            UPDATE alerts
            SET is_triggered = $1,
                last_triggered_at = COALESCE($2, last_triggered_at),
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $3
            "#,
        )
        .bind(is_triggered)
        .bind(last_triggered_at)
        .bind(alert_id)
        .execute(self.0)
        .await?;

        Ok(())
    }
}

/// Unified Database Repository holding a pool reference and providing
/// direct access to individual sub-repositories.
#[derive(Debug, Clone)]
pub struct DbRepository {
    pool: PgPool,
}

impl DbRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    #[allow(dead_code)]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn guilds(&self) -> GuildRepository<'_> {
        GuildRepository(&self.pool)
    }

    pub fn channels(&self) -> ChannelRepository<'_> {
        ChannelRepository(&self.pool)
    }

    pub fn users(&self) -> UserRepository<'_> {
        UserRepository(&self.pool)
    }

    pub fn alerts(&self) -> AlertRepository<'_> {
        AlertRepository(&self.pool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_database_repository_lifecycle() {
        dotenvy::dotenv().ok();
        let db_url = match std::env::var("DATABASE_URL") {
            Ok(url) if !url.is_empty() => url,
            _ => {
                println!("Skipping live DB test: DATABASE_URL not set");
                return;
            }
        };

        let pool = match init_db(&db_url).await {
            Ok(p) => p,
            Err(e) => {
                println!("Skipping live DB test (connect failed: {e})");
                return;
            }
        };

        if let Err(e) = run_migrations(&pool).await {
            println!("Skipping live DB test (migrations failed: {e})");
            return;
        }

        let repo = DbRepository::new(pool);

        let test_guild_id = "test_guild_999999";
        let test_channel_id = "test_channel_999999";
        let test_user_id = "test_user_999999";

        // 1. Guild upsert
        let guild_id = repo
            .guilds()
            .upsert(test_guild_id, "Test Integration Guild")
            .await
            .expect("Upsert guild failed");
        assert!(guild_id > 0);

        // 2. Channel upsert
        let channel_id = repo
            .channels()
            .upsert(Some(guild_id), test_channel_id, "test-channel")
            .await
            .expect("Upsert channel failed");
        assert!(channel_id > 0);

        // 3. User upsert
        let user_id = repo
            .users()
            .upsert(test_user_id, "TestIntegrationUser")
            .await
            .expect("Upsert user failed");
        assert!(user_id > 0);

        // 4. Create Alert
        let alert = repo
            .alerts()
            .create(
                user_id,
                channel_id,
                "SOLUSDT",
                "price_above",
                dec!(300.00),
                None,
                1800,
            )
            .await
            .expect("Create alert failed");
        assert!(alert.id > 0);
        assert_eq!(alert.symbol, "SOLUSDT");

        // 5. Get user alerts
        let user_alerts = repo
            .alerts()
            .get_user_alerts(test_user_id)
            .await
            .expect("Get user alerts failed");
        let found = user_alerts.iter().find(|a| a.id == alert.id);
        assert!(found.is_some(), "Expected created alert to be found");

        // 6. Update trigger state
        let now = chrono::Utc::now().naive_utc();
        repo.alerts()
            .update_trigger_state(alert.id, true, Some(now))
            .await
            .expect("Update trigger state failed");

        // 7. Delete Alert
        let deleted = repo
            .alerts()
            .delete(alert.id, test_user_id)
            .await
            .expect("Delete alert failed");
        assert!(deleted, "Expected alert to be deleted");

        // Verify it is gone
        let user_alerts_after = repo
            .alerts()
            .get_user_alerts(test_user_id)
            .await
            .expect("Get user alerts failed");
        assert!(user_alerts_after.iter().all(|a| a.id != alert.id));
    }
}