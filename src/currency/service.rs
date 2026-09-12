use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::currency::models::{find_currency, CurrencyInfo, SUPPORTED_CURRENCIES};

/// Free, open exchange rate API endpoint (no API key required)
const OPEN_EXCHANGE_RATE_URL: &str = "https://open.er-api.com/v6/latest/USD";
const REDIS_CURRENCY_CACHE_KEY: &str = "binbot:currency:rates";

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ApiResponse {
    result: Option<String>,
    rates: Option<HashMap<String, serde_json::Value>>,
}

/// Persisted cache structure in Redis
#[derive(Debug, Serialize, Deserialize)]
struct CachedRatesPayload {
    updated_at: DateTime<Utc>,
    rates: HashMap<String, Decimal>,
}

/// Information about a detected currency difference
#[derive(Debug, Clone, PartialEq)]
pub struct RateDifference {
    pub code: String,
    pub old_rate: Decimal,
    pub new_rate: Decimal,
    pub change_percent: Decimal,
}

/// Outcome of an interval-based cache check
#[derive(Debug, Clone, PartialEq)]
pub enum CacheCheckResult {
    /// Rates are identical or differences are below threshold; cached values kept
    Unchanged { checked_count: usize },
    /// Significant differences detected; cache was updated
    Updated {
        updated_count: usize,
        changes: Vec<RateDifference>,
    },
}

/// Service managing fiat currency exchange rates relative to USD with caching & diff checks
#[derive(Clone)]
pub struct CurrencyService {
    rates: Arc<RwLock<HashMap<String, Decimal>>>,
    last_updated: Arc<RwLock<DateTime<Utc>>>,
    last_checked: Arc<RwLock<DateTime<Utc>>>,
    diff_threshold_percent: Decimal,
    redis: Option<Arc<Mutex<MultiplexedConnection>>>,
    client: reqwest::Client,
}

impl Default for CurrencyService {
    fn default() -> Self {
        Self::new(None, dec!(0.1))
    }
}

impl std::fmt::Debug for CurrencyService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CurrencyService")
            .field("diff_threshold_percent", &self.diff_threshold_percent)
            .finish()
    }
}

impl CurrencyService {
    /// Creates a new `CurrencyService` with optional Redis connection and diff threshold
    pub fn new(
        redis_conn: Option<MultiplexedConnection>,
        diff_threshold_percent: Decimal,
    ) -> Self {
        let mut initial_rates = HashMap::new();

        // Standard baseline rates (1 USD = X) as emergency fallback
        initial_rates.insert("USD".to_string(), dec!(1.0));
        initial_rates.insert("PHP".to_string(), dec!(58.50));
        initial_rates.insert("CAD".to_string(), dec!(1.38));
        initial_rates.insert("JPY".to_string(), dec!(155.00));
        initial_rates.insert("EUR".to_string(), dec!(0.92));
        initial_rates.insert("GBP".to_string(), dec!(0.79));
        initial_rates.insert("AUD".to_string(), dec!(1.52));
        initial_rates.insert("SGD".to_string(), dec!(1.32));
        initial_rates.insert("INR".to_string(), dec!(85.50));
        initial_rates.insert("BRL".to_string(), dec!(5.70));
        initial_rates.insert("CHF".to_string(), dec!(0.88));
        initial_rates.insert("NZD".to_string(), dec!(1.68));
        initial_rates.insert("HKD".to_string(), dec!(7.77));
        initial_rates.insert("KRW".to_string(), dec!(1400.00));
        initial_rates.insert("THB".to_string(), dec!(34.50));
        initial_rates.insert("IDR".to_string(), dec!(16000.00));
        initial_rates.insert("VND".to_string(), dec!(25400.00));
        initial_rates.insert("MXN".to_string(), dec!(20.20));
        initial_rates.insert("AED".to_string(), dec!(3.67));

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Binbot/0.1.0")
            .build()
            .unwrap_or_default();

        let now = Utc::now();

        Self {
            rates: Arc::new(RwLock::new(initial_rates)),
            last_updated: Arc::new(RwLock::new(now)),
            last_checked: Arc::new(RwLock::new(now)),
            diff_threshold_percent,
            redis: redis_conn.map(|c| Arc::new(Mutex::new(c))),
            client,
        }
    }

    /// Initializes cache on startup:
    /// 1. Tries to restore cached values from Redis.
    /// 2. If Redis has no cache, fetches fresh rates from API and seeds Redis.
    /// 3. Falls back to baseline rates only if both Redis and API are unreachable.
    pub async fn init_cache(&self) {
        if let Some(restored) = self.load_from_redis().await {
            tracing::info!(
                "✓ Restored {} currency rates from Redis cache (last updated: {})",
                restored.rates.len(),
                restored.updated_at.to_rfc3339()
            );
            *self.rates.write().await = restored.rates;
            *self.last_updated.write().await = restored.updated_at;
            *self.last_checked.write().await = Utc::now();
            return;
        }

        tracing::info!("No existing Redis currency cache found; querying open API to seed cache...");
        match self.fetch_candidate_rates().await {
            Ok(fresh_rates) => {
                let count = fresh_rates.len();
                let now = Utc::now();
                *self.rates.write().await = fresh_rates.clone();
                *self.last_updated.write().await = now;
                *self.last_checked.write().await = now;
                self.save_to_redis(&fresh_rates, now).await;
                tracing::info!("✓ Successfully seeded currency cache with {} rates from API", count);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch initial rates from API ({}); using baseline rates as fallback",
                    e
                );
            }
        }
    }

    /// Retrieves the exchange rate for a given currency code relative to 1 USD
    pub async fn get_rate(&self, code: &str) -> Option<Decimal> {
        let code_up = code.trim().to_uppercase();
        if code_up == "USD" {
            return Some(Decimal::ONE);
        }
        let map = self.rates.read().await;
        map.get(&code_up).copied()
    }

    /// Resolves user query into CurrencyInfo and its conversion rate
    pub async fn resolve_currency(&self, query: &str) -> Option<(CurrencyInfo, Decimal)> {
        let info = find_currency(query)?;
        let rate = self.get_rate(info.code).await?;
        Some((info, rate))
    }

    /// Converts an amount in USD to the target currency
    #[allow(dead_code)]
    pub fn convert(&self, usd_amount: Decimal, rate: Decimal) -> Decimal {
        usd_amount * rate
    }

    /// Formats a fiat amount with commas, currency symbol, and standard decimal places
    pub fn format_amount(amount: Decimal, currency: &CurrencyInfo) -> String {
        let is_negative = amount < Decimal::ZERO;
        let abs_val = amount.abs();

        let formatted_number = if currency.decimals == 0 {
            let int_val: i64 = abs_val.to_string().split('.').next().unwrap_or("0").parse().unwrap_or(0);
            insert_commas(&int_val.to_string())
        } else if abs_val >= Decimal::from(1) {
            let rounded = format!("{:.2}", abs_val);
            let parts: Vec<&str> = rounded.split('.').collect();
            let int_part = insert_commas(parts[0]);
            let dec_part = parts.get(1).unwrap_or(&"00");
            format!("{}.{}", int_part, dec_part)
        } else {
            // For small decimal values (< 1)
            format!("{:.4}", abs_val)
        };

        let sign = if is_negative { "-" } else { "" };
        format!("{}{}{}", sign, currency.symbol, formatted_number)
    }

    /// Fetches candidate exchange rates from the external API without applying them immediately
    async fn fetch_candidate_rates(&self) -> Result<HashMap<String, Decimal>, String> {
        let res = self
            .client
            .get(OPEN_EXCHANGE_RATE_URL)
            .send()
            .await
            .map_err(|e| format!("Network request failed: {e}"))?;

        if !res.status().is_success() {
            return Err(format!("API returned HTTP status: {}", res.status()));
        }

        let body = res
            .json::<ApiResponse>()
            .await
            .map_err(|e| format!("Failed to parse API response JSON: {e}"))?;

        let rates_raw = body.rates.ok_or_else(|| "No rates field found in response".to_string())?;
        let mut parsed = HashMap::new();

        for (code, val) in rates_raw {
            let dec_opt = match val {
                serde_json::Value::Number(n) => {
                    if let Some(f) = n.as_f64() {
                        Decimal::try_from(f).ok()
                    } else {
                        None
                    }
                }
                serde_json::Value::String(s) => s.parse::<Decimal>().ok(),
                _ => None,
            };

            if let Some(dec_rate) = dec_opt {
                parsed.insert(code.to_uppercase(), dec_rate);
            }
        }

        Ok(parsed)
    }

    /// Compares candidate rates against current cached rates and returns all differences exceeding threshold
    pub async fn detect_differences(&self, candidate_rates: &HashMap<String, Decimal>) -> Vec<RateDifference> {
        let current_rates = self.rates.read().await;
        let mut differences = Vec::new();

        for curr in SUPPORTED_CURRENCIES {
            if curr.code == "USD" {
                continue;
            }

            if let Some(&new_rate) = candidate_rates.get(curr.code) {
                if let Some(&old_rate) = current_rates.get(curr.code) {
                    if old_rate > Decimal::ZERO {
                        let diff = (new_rate - old_rate).abs();
                        let percent_change = (diff / old_rate) * Decimal::from(100);

                        if percent_change >= self.diff_threshold_percent {
                            differences.push(RateDifference {
                                code: curr.code.to_string(),
                                old_rate,
                                new_rate,
                                change_percent: (new_rate - old_rate) / old_rate * Decimal::from(100),
                            });
                        }
                    }
                }
            }
        }

        differences
    }

    /// Performs an interval check comparing current cached values against fresh external API values.
    /// Only updates the cache if significant differences are detected.
    pub async fn check_and_update_rates(&self) -> Result<CacheCheckResult, String> {
        tracing::info!("Running interval currency rate check against external API...");

        let candidate_rates = self.fetch_candidate_rates().await?;
        let now = Utc::now();
        *self.last_checked.write().await = now;

        let differences = self.detect_differences(&candidate_rates).await;

        if differences.is_empty() {
            tracing::info!(
                "✓ Currency check complete: rates are identical (no change >= {}%). Keeping existing cache.",
                self.diff_threshold_percent
            );
            Ok(CacheCheckResult::Unchanged {
                checked_count: candidate_rates.len(),
            })
        } else {
            tracing::info!(
                "✓ Rate difference detected for {} currencies; updating cache:",
                differences.len()
            );
            for diff in &differences {
                tracing::info!(
                    "   - {}: {} -> {} ({:+.2}%)",
                    diff.code,
                    diff.old_rate,
                    diff.new_rate,
                    diff.change_percent
                );
            }

            // Update in-memory cache with full fresh dataset
            *self.rates.write().await = candidate_rates.clone();
            *self.last_updated.write().await = now;

            // Persist updated cache to Redis
            self.save_to_redis(&candidate_rates, now).await;

            Ok(CacheCheckResult::Updated {
                updated_count: candidate_rates.len(),
                changes: differences,
            })
        }
    }

    /// Loads cached rates payload from Redis if present
    async fn load_from_redis(&self) -> Option<CachedRatesPayload> {
        let redis_mutex = self.redis.as_ref()?;
        let mut conn = redis_mutex.lock().await;

        let raw: Option<String> = conn.get(REDIS_CURRENCY_CACHE_KEY).await.ok()?;
        let payload_str = raw?;

        serde_json::from_str::<CachedRatesPayload>(&payload_str).ok()
    }

    /// Persists rates and update timestamp to Redis cache
    async fn save_to_redis(&self, rates: &HashMap<String, Decimal>, updated_at: DateTime<Utc>) {
        if let Some(ref redis_mutex) = self.redis {
            let payload = CachedRatesPayload {
                updated_at,
                rates: rates.clone(),
            };

            if let Ok(json_str) = serde_json::to_string(&payload) {
                let mut conn = redis_mutex.lock().await;
                let res: Result<(), redis::RedisError> = conn.set(REDIS_CURRENCY_CACHE_KEY, json_str).await;
                if let Err(e) = res {
                    tracing::warn!("Failed to persist currency cache to Redis: {}", e);
                } else {
                    tracing::info!("✓ Currency cache persisted to Redis under key '{}'", REDIS_CURRENCY_CACHE_KEY);
                }
            }
        }
    }

    /// Spawns the background task that triggers the mandatory interval comparison
    pub fn spawn_interval_checker(self: Arc<Self>, interval: Duration) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            // Skip immediate tick because init_cache already ran
            ticker.tick().await;

            loop {
                ticker.tick().await;
                if let Err(e) = self.check_and_update_rates().await {
                    tracing::warn!("Interval currency check failed: {}", e);
                }
            }
        });
    }

    /// Retrieves all supported currencies along with their current exchange rate
    pub async fn get_supported_rates(&self) -> Vec<(CurrencyInfo, Decimal)> {
        let map = self.rates.read().await;
        SUPPORTED_CURRENCIES
            .iter()
            .map(|c| {
                let rate = map.get(c.code).copied().unwrap_or(Decimal::ONE);
                (*c, rate)
            })
            .collect()
    }

    /// Timestamp of when the cache was last updated with new values
    pub async fn last_updated(&self) -> DateTime<Utc> {
        *self.last_updated.read().await
    }

    /// Timestamp of when the cache was last checked against the external API
    pub async fn last_checked(&self) -> DateTime<Utc> {
        *self.last_checked.read().await
    }
}

/// Helper function to format an integer string with comma separators
fn insert_commas(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();

    for (i, &c) in chars.iter().enumerate() {
        result.push(c);
        let remaining = len - 1 - i;
        if remaining > 0 && remaining % 3 == 0 {
            result.push(',');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_currency_conversion_and_formatting() {
        let service = CurrencyService::default();

        // Test USD rate
        let usd_rate = service.get_rate("USD").await;
        assert_eq!(usd_rate, Some(dec!(1.0)));

        // Test PHP rate & resolution
        let (php_info, php_rate) = service.resolve_currency("php").await.unwrap();
        assert_eq!(php_info.code, "PHP");
        assert_eq!(php_info.symbol, "₱");
        assert_eq!(php_rate, dec!(58.50));

        // Test conversion: 100 USD -> PHP
        let converted = service.convert(dec!(100.00), php_rate);
        assert_eq!(converted, dec!(5850.000));
        assert_eq!(CurrencyService::format_amount(converted, &php_info), "₱5,850.00");

        // Test JPY resolution and zero decimals formatting
        let (jpy_info, jpy_rate) = service.resolve_currency("JPY").await.unwrap();
        assert_eq!(jpy_info.code, "JPY");
        assert_eq!(jpy_info.symbol, "¥");
        let jpy_amt = service.convert(dec!(1000.00), jpy_rate);
        assert_eq!(CurrencyService::format_amount(jpy_amt, &jpy_info), "¥155,000");

        // Test Canadian Dollar
        let (cad_info, _) = service.resolve_currency("cad").await.unwrap();
        assert_eq!(cad_info.symbol, "C$");
        assert_eq!(CurrencyService::format_amount(dec!(12345.67), &cad_info), "C$12,345.67");
    }

    #[tokio::test]
    async fn test_cache_payload_serde() {
        let mut rates = HashMap::new();
        rates.insert("PHP".to_string(), dec!(58.50));
        rates.insert("CAD".to_string(), dec!(1.38));

        let payload = CachedRatesPayload {
            updated_at: Utc::now(),
            rates,
        };

        let json = serde_json::to_string(&payload).unwrap();
        let decoded: CachedRatesPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.rates.get("PHP"), Some(&dec!(58.50)));
        assert_eq!(decoded.rates.get("CAD"), Some(&dec!(1.38)));
    }

    #[tokio::test]
    async fn test_differential_detection() {
        let service = CurrencyService::new(None, dec!(0.5)); // 0.5% threshold

        // Identical rates should detect zero differences
        let mut candidate = HashMap::new();
        candidate.insert("PHP".to_string(), dec!(58.50));
        candidate.insert("CAD".to_string(), dec!(1.38));
        let diffs = service.detect_differences(&candidate).await;
        assert!(diffs.is_empty());

        // Minor change below 0.5% (e.g. 58.50 -> 58.60 = 0.17%) should not trigger
        candidate.insert("PHP".to_string(), dec!(58.60));
        let diffs = service.detect_differences(&candidate).await;
        assert!(diffs.is_empty());

        // Major change above 0.5% (e.g. 58.50 -> 59.50 = 1.7%) should trigger
        candidate.insert("PHP".to_string(), dec!(59.50));
        let diffs = service.detect_differences(&candidate).await;
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].code, "PHP");
        assert_eq!(diffs[0].old_rate, dec!(58.50));
        assert_eq!(diffs[0].new_rate, dec!(59.50));
    }
}
