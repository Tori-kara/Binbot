use std::sync::Arc;
use std::time::Duration;
use chrono::{DateTime, Timelike, Utc};
use rust_decimal::Decimal;
use serenity::all::{ChannelId, Http};
use tracing::{error, info, warn};

use crate::discord::embeds::create_market_overview_embed;
use crate::market::models::MarketData;
use crate::market::state::MarketState;
use crate::storage::db::DbRepository;

/// Consolidated market digest data model for global summary and daily digests
#[derive(Debug, Clone, PartialEq)]
pub struct MarketDigest {
    pub tracked_count: usize,
    pub green_count: usize,
    pub red_count: usize,
    pub top_gainers: Vec<MarketData>,
    pub top_losers: Vec<MarketData>,
    pub volume_leaders: Vec<MarketData>,
    pub benchmarks: Vec<MarketData>,
    pub generated_at: DateTime<Utc>,
}

/// Pure function compiling a `MarketDigest` from all available market snapshots
pub fn compile_market_digest(snapshots: &[MarketData], now: DateTime<Utc>) -> MarketDigest {
    let mut tracked = snapshots.to_vec();
    let tracked_count = tracked.len();

    let mut green_count = 0;
    let mut red_count = 0;
    for item in &tracked {
        if item.price_change_percent_24hr >= Decimal::ZERO {
            green_count += 1;
        } else {
            red_count += 1;
        }
    }

    // Sort by 24h % change descending for gainers
    tracked.sort_by(|a, b| b.price_change_percent_24hr.cmp(&a.price_change_percent_24hr));
    let top_gainers = tracked.iter().take(3).cloned().collect();

    // Sort by 24h % change ascending for losers
    let mut losers_sort = tracked.clone();
    losers_sort.sort_by(|a, b| a.price_change_percent_24hr.cmp(&b.price_change_percent_24hr));
    let top_losers = losers_sort.iter().take(3).cloned().collect();

    // Sort by quote volume descending for volume leaders
    let mut volume_sort = tracked.clone();
    volume_sort.sort_by(|a, b| b.quote_volume_24hr.cmp(&a.quote_volume_24hr));
    let volume_leaders = volume_sort.iter().take(3).cloned().collect();

    // Benchmark assets: BTC, ETH, SOL
    let benchmark_symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT"];
    let mut benchmarks = Vec::new();
    for sym in &benchmark_symbols {
        if let Some(found) = tracked.iter().find(|m| m.symbol.eq_ignore_ascii_case(sym)) {
            benchmarks.push(found.clone());
        }
    }

    MarketDigest {
        tracked_count,
        green_count,
        red_count,
        top_gainers,
        top_losers,
        volume_leaders,
        benchmarks,
        generated_at: now,
    }
}

/// Computes the duration in seconds until the next 08:00:00 UTC
pub fn duration_until_next_target_utc(target_hour: u32, target_minute: u32) -> Duration {
    let now = Utc::now();
    let current_hour = now.hour();
    let current_minute = now.minute();
    let current_second = now.second();

    let now_secs = current_hour * 3600 + current_minute * 60 + current_second;
    let target_secs = target_hour * 3600 + target_minute * 60;

    let diff_secs = if now_secs < target_secs {
        target_secs - now_secs
    } else {
        (86400 - now_secs) + target_secs
    };

    Duration::from_secs(diff_secs as u64)
}

/// Service managing scheduled daily market digest broadcasts
#[derive(Debug, Clone)]
pub struct MarketDigestScheduler {
    market_state: MarketState,
    repo: DbRepository,
    http: Arc<Http>,
    target_utc_hour: u32,
    target_utc_minute: u32,
}

impl MarketDigestScheduler {
    pub fn new(
        market_state: MarketState,
        repo: DbRepository,
        http: Arc<Http>,
    ) -> Self {
        Self {
            market_state,
            repo,
            http,
            target_utc_hour: 8,
            target_utc_minute: 0,
        }
    }

    #[allow(dead_code)]
    pub fn with_target_time(mut self, hour: u32, minute: u32) -> Self {
        self.target_utc_hour = hour;
        self.target_utc_minute = minute;
        self
    }

    /// Spawns the background cron loop broadcasting daily digests at 08:00 UTC
    pub async fn run(self) {
        info!(
            "✓ Market Digest Scheduler started (target: {:02}:{:02} UTC daily)",
            self.target_utc_hour, self.target_utc_minute
        );

        loop {
            let wait_duration = duration_until_next_target_utc(self.target_utc_hour, self.target_utc_minute);
            info!(
                "Next scheduled market digest in {:.1} hours ({:02}:{:02} UTC)",
                wait_duration.as_secs_f64() / 3600.0,
                self.target_utc_hour,
                self.target_utc_minute
            );

            tokio::time::sleep(wait_duration).await;

            // Execute broadcast
            self.broadcast_daily_digest().await;

            // Prevent double trigger in the same minute
            tokio::time::sleep(Duration::from_secs(65)).await;
        }
    }

    /// Broadcasts the daily market summary embed to all subscribed channels
    pub async fn broadcast_daily_digest(&self) {
        let subscriptions = match self.repo.digests().get_active_subscriptions().await {
            Ok(subs) => subs,
            Err(e) => {
                error!("Failed to query digest subscriptions: {e}");
                return;
            }
        };

        if subscriptions.is_empty() {
            info!("No channels subscribed to daily market digest at 08:00 UTC");
            return;
        }

        let snapshots = self.market_state.get_all_snapshots().await;
        if snapshots.is_empty() {
            warn!("No market data snapshots available for daily digest");
            return;
        }

        let digest = compile_market_digest(&snapshots, Utc::now());
        let embed = create_market_overview_embed(&digest, true);

        info!(
            "Broadcasting daily market digest to {} subscribed channel(s)",
            subscriptions.len()
        );

        for sub in subscriptions {
            let channel_id_num = match sub.channel_discord_id.parse::<u64>() {
                Ok(id) => id,
                Err(_) => {
                    warn!("Invalid channel discord ID: {}", sub.channel_discord_id);
                    continue;
                }
            };

            let channel_id = ChannelId::new(channel_id_num);
            let embed_clone = embed.clone();

            let builder = serenity::all::CreateMessage::new().embed(embed_clone);

            match channel_id.send_message(&self.http, builder).await {
                Ok(_) => {
                    info!("✓ Sent daily market digest to channel #{}", sub.channel_discord_id);
                }
                Err(e) => {
                    warn!(
                        "Failed to send digest to channel #{}: {e}. (Bot may lack send message permissions)",
                        sub.channel_discord_id
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_compile_market_digest() {
        let now = Utc::now();
        let snapshots = vec![
            MarketData {
                symbol: "BTCUSDT".to_string(),
                price: dec!(100000),
                price_change_24hr: dec!(2000),
                price_change_percent_24hr: dec!(2.0),
                high_price_24hr: dec!(101000),
                low_price_24hr: dec!(98000),
                volume_24hr: dec!(10000),
                quote_volume_24hr: dec!(1000000000),
                update_at: now,
            },
            MarketData {
                symbol: "ETHUSDT".to_string(),
                price: dec!(3500),
                price_change_24hr: dec!(350),
                price_change_percent_24hr: dec!(11.1),
                high_price_24hr: dec!(3600),
                low_price_24hr: dec!(3100),
                volume_24hr: dec!(50000),
                quote_volume_24hr: dec!(175000000),
                update_at: now,
            },
            MarketData {
                symbol: "SOLUSDT".to_string(),
                price: dec!(200),
                price_change_24hr: dec!(-20),
                price_change_percent_24hr: dec!(-9.09),
                high_price_24hr: dec!(220),
                low_price_24hr: dec!(190),
                volume_24hr: dec!(100000),
                quote_volume_24hr: dec!(20000000),
                update_at: now,
            },
            MarketData {
                symbol: "DOGEUSDT".to_string(),
                price: dec!(0.20),
                price_change_24hr: dec!(-0.04),
                price_change_percent_24hr: dec!(-16.6),
                high_price_24hr: dec!(0.24),
                low_price_24hr: dec!(0.19),
                volume_24hr: dec!(1000000),
                quote_volume_24hr: dec!(200000),
                update_at: now,
            },
        ];

        let digest = compile_market_digest(&snapshots, now);
        assert_eq!(digest.tracked_count, 4);
        assert_eq!(digest.green_count, 2);
        assert_eq!(digest.red_count, 2);

        // Top gainer: ETH (+11.1%)
        assert_eq!(digest.top_gainers[0].symbol, "ETHUSDT");
        // Top loser: DOGE (-16.6%)
        assert_eq!(digest.top_losers[0].symbol, "DOGEUSDT");
        // Volume leader: BTC ($1B quote)
        assert_eq!(digest.volume_leaders[0].symbol, "BTCUSDT");
        // Benchmarks found
        assert_eq!(digest.benchmarks.len(), 3);
    }

    #[test]
    fn test_duration_calculation() {
        let dur = duration_until_next_target_utc(8, 0);
        assert!(dur.as_secs() > 0 && dur.as_secs() <= 86400);
    }
}
