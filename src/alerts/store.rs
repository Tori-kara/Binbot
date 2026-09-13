use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};

use crate::alerts::cooldown::CooldownTracker;
use crate::alerts::models::{Alert, AlertCondition};
use crate::storage::db::DbRepository;
use crate::storage::redis::RedisStore;

/// Thread-safe alert storage managing PostgreSQL persistence via Repository Pattern,
/// distributed cooldown locks via Redis, and an in-memory symbol-indexed cache.
#[derive(Debug, Clone)]
pub struct AlertStore {
    repo: DbRepository,
    redis: Option<RedisStore>,
    cache: Arc<RwLock<HashMap<String, Vec<Alert>>>>,
}

impl AlertStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: DbRepository::new(pool),
            redis: None,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Attaches a Redis store for distributed cooldown locks
    pub fn with_redis(mut self, redis: RedisStore) -> Self {
        self.redis = Some(redis);
        self
    }

    /// Access the underlying database pool
    #[allow(dead_code)]
    pub fn pool(&self) -> &PgPool {
        self.repo.pool()
    }

    /// Access the database repository
    #[allow(dead_code)]
    pub fn repo(&self) -> &DbRepository {
        &self.repo
    }

    /// Access the optional Redis store
    #[allow(dead_code)]
    pub fn redis(&self) -> Option<&RedisStore> {
        self.redis.as_ref()
    }

    /// Loads all active (enabled) alerts from PostgreSQL into the in-memory cache
    pub async fn load_all_active_alerts(&self) -> Result<usize, sqlx::Error> {
        let rows = self.repo.alerts().get_active_alerts().await?;

        let mut map: HashMap<String, Vec<Alert>> = HashMap::new();
        let total = rows.len();

        for row in rows {
            let id = row.id;
            let symbol = row.symbol;
            let condition = match AlertCondition::from_parts(&row.condition_type, row.threshold) {
                Ok(c) => c,
                Err(e) => {
                    error!("Skipping corrupt alert #{id}: {e}");
                    continue;
                }
            };

            let alert = Alert {
                id,
                user_id: row.user_id,
                user_discord_id: row.user_discord_id,
                channel_id: row.channel_id,
                channel_discord_id: row.channel_discord_id,
                symbol: symbol.clone(),
                condition,
                threshold: row.threshold,
                baseline_price: row.baseline_price,
                cooldown_seconds: row.cooldown_seconds as u32,
                last_triggered_at: row.last_triggered_at.map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)),
                is_triggered: row.is_triggered,
                enabled: row.enabled,
                created_at: DateTime::<Utc>::from_naive_utc_and_offset(row.created_at, Utc),
                updated_at: DateTime::<Utc>::from_naive_utc_and_offset(row.updated_at, Utc),
            };

            map.entry(symbol.to_uppercase()).or_default().push(alert);
        }

        let mut cache = self.cache.write().await;
        *cache = map;
        info!("✓ Loaded {total} active alerts into in-memory engine cache from PostgreSQL");

        Ok(total)
    }

    /// Creates a new alert, upserting guild, channel, and user in PostgreSQL via repositories,
    /// and registers the alert into the in-memory engine cache.
    pub async fn create_alert(
        &self,
        guild_discord_id: Option<&str>,
        guild_name: Option<&str>,
        channel_discord_id: &str,
        channel_name: &str,
        user_discord_id: &str,
        username: &str,
        symbol: &str,
        condition: AlertCondition,
        baseline_price: Option<Decimal>,
        cooldown_seconds: u32,
    ) -> Result<Alert, sqlx::Error> {
        let clean_symbol = symbol.trim().to_uppercase();

        // 1. Ensure Guild exists (if provided)
        let db_guild_id = if let Some(gid) = guild_discord_id {
            let name = guild_name.unwrap_or("Discord Server");
            Some(self.repo.guilds().upsert(gid, name).await?)
        } else {
            None
        };

        // 2. Ensure Channel exists
        let db_channel_id = self
            .repo
            .channels()
            .upsert(db_guild_id, channel_discord_id, channel_name)
            .await?;

        // 3. Ensure User exists
        let db_user_id = self
            .repo
            .users()
            .upsert(user_discord_id, username)
            .await?;

        // 4. Insert Alert in PostgreSQL
        let condition_type = condition.condition_type_str();
        let threshold = condition.threshold_value();

        let alert_row = self
            .repo
            .alerts()
            .create(
                db_user_id,
                db_channel_id,
                &clean_symbol,
                condition_type,
                threshold,
                baseline_price,
                cooldown_seconds as i32,
            )
            .await?;

        let alert = Alert {
            id: alert_row.id,
            user_id: db_user_id,
            user_discord_id: user_discord_id.to_string(),
            channel_id: db_channel_id,
            channel_discord_id: channel_discord_id.to_string(),
            symbol: clean_symbol.clone(),
            condition,
            threshold,
            baseline_price,
            cooldown_seconds,
            last_triggered_at: None,
            is_triggered: false,
            enabled: true,
            created_at: DateTime::<Utc>::from_naive_utc_and_offset(alert_row.created_at, Utc),
            updated_at: DateTime::<Utc>::from_naive_utc_and_offset(alert_row.updated_at, Utc),
        };

        // 5. Update in-memory cache
        let mut cache = self.cache.write().await;
        cache
            .entry(clean_symbol)
            .or_default()
            .push(alert.clone());

        debug!(alert_id = alert.id, symbol = %alert.symbol, "Alert registered successfully");

        Ok(alert)
    }

    /// Fetches all active alerts for a given symbol from the in-memory cache
    pub async fn get_alerts_for_symbol(&self, symbol: &str) -> Vec<Alert> {
        let cache = self.cache.read().await;
        cache.get(&symbol.to_uppercase()).cloned().unwrap_or_default()
    }

    /// Fetches all alerts belonging to a specific Discord user
    pub async fn get_alerts_for_user(&self, user_discord_id: &str) -> Vec<Alert> {
        let cache = self.cache.read().await;
        let mut result = Vec::new();
        for alerts in cache.values() {
            for alert in alerts {
                if alert.user_discord_id == user_discord_id {
                    result.push(alert.clone());
                }
            }
        }
        result.sort_by_key(|a| a.id);
        result
    }

    /// Deletes an alert by ID, ensuring ownership, removing from memory and clearing Redis cooldown
    pub async fn delete_alert(&self, alert_id: i64, user_discord_id: &str) -> Result<bool, sqlx::Error> {
        let deleted = self.repo.alerts().delete(alert_id, user_discord_id).await?;

        if deleted {
            // Remove from in-memory cache
            let mut cache = self.cache.write().await;
            for alerts in cache.values_mut() {
                alerts.retain(|a| a.id != alert_id);
            }

            // Clear Redis cooldown key if active
            if let Some(redis) = &self.redis {
                let _ = redis.clear_cooldown(alert_id).await;
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Updates the trigger state and last_triggered_at timestamp of an alert
    pub async fn update_trigger_state(
        &self,
        alert_id: i64,
        symbol: &str,
        is_triggered: bool,
        last_triggered_at: Option<DateTime<Utc>>,
    ) {
        // 1. Update in-memory cache immediately
        {
            let mut cache = self.cache.write().await;
            if let Some(alerts) = cache.get_mut(&symbol.to_uppercase()) {
                for a in alerts.iter_mut() {
                    if a.id == alert_id {
                        a.is_triggered = is_triggered;
                        if last_triggered_at.is_some() {
                            a.last_triggered_at = last_triggered_at;
                        }
                        break;
                    }
                }
            }
        }

        // 2. Persist to PostgreSQL in background via repository
        let repo = self.repo.clone();
        let naive_last = last_triggered_at.map(|dt| dt.naive_utc());
        tokio::spawn(async move {
            let res = repo.alerts().update_trigger_state(alert_id, is_triggered, naive_last).await;
            if let Err(e) = res {
                error!("Failed to update trigger state in DB for alert #{alert_id}: {e}");
            }
        });
    }

    /// Attempts to acquire an atomic distributed cooldown lock in Redis:
    /// `SET cooldown:{alert_id} 1 EX {cooldown_seconds} NX`.
    /// Returns `true` if lock acquired, `false` if currently in cooldown.
    /// Falls back to local in-memory tracker if Redis is not configured or errors out.
    pub async fn try_acquire_cooldown(
        &self,
        alert_id: i64,
        cooldown_seconds: u32,
        last_triggered_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> bool {
        if let Some(redis) = &self.redis {
            match redis.try_set_cooldown(alert_id, cooldown_seconds).await {
                Ok(acquired) => return acquired,
                Err(e) => {
                    warn!("Redis cooldown check failed for alert #{alert_id}: {e}, falling back to memory");
                }
            }
        }

        !CooldownTracker::is_cooling_down(last_triggered_at, cooldown_seconds, now)
    }

    /// Checks if an alert is cooling down, verifying Redis first with in-memory fallback.
    pub async fn is_cooling_down(
        &self,
        alert_id: i64,
        last_triggered_at: Option<DateTime<Utc>>,
        cooldown_seconds: u32,
        now: DateTime<Utc>,
    ) -> bool {
        if let Some(redis) = &self.redis {
            match redis.is_cooling_down(alert_id).await {
                Ok(cooling) => return cooling,
                Err(e) => {
                    trace!("Redis is_cooling_down check failed for alert #{alert_id}: {e}");
                }
            }
        }

        CooldownTracker::is_cooling_down(last_triggered_at, cooldown_seconds, now)
    }

    /// Computes remaining cooldown duration, querying Redis TTL first with in-memory fallback.
    pub async fn get_cooldown_remaining(
        &self,
        alert_id: i64,
        last_triggered_at: Option<DateTime<Utc>>,
        cooldown_seconds: u32,
        now: DateTime<Utc>,
    ) -> Option<Duration> {
        if let Some(redis) = &self.redis {
            if let Ok(Some(rem)) = redis.get_cooldown_remaining(alert_id).await {
                return Some(rem);
            }
        }

        CooldownTracker::time_remaining(last_triggered_at, cooldown_seconds, now)
    }
}

