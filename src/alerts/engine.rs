use std::sync::Arc;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, trace, warn};

use crate::alerts::cooldown::CooldownTracker;
use crate::alerts::models::{Alert, AlertCondition, ComparisonOp, MetricTarget, DEFAULT_HYSTERESIS_RATE};
use crate::alerts::store::AlertStore;
use crate::market::models::{MarketData, MarketUpdateEvent};
use crate::market::rolling::RollingMoveStats;
use crate::market::state::MarketState;

/// Domain event representing an alert notification ready for Discord dispatch
#[derive(Debug, Clone)]
pub struct AlertNotification {
    pub alert_id: i64,
    pub user_discord_id: String,
    pub channel_discord_id: String,
    pub symbol: String,
    pub condition: AlertCondition,
    pub trigger_price: Decimal,
    pub previous_price: Option<Decimal>,
    pub baseline_price: Option<Decimal>,
    pub triggered_at: DateTime<Utc>,
}

/// The result of evaluating an alert against a market price update
#[derive(Debug, Clone, PartialEq)]
pub enum AlertEvaluation {
    /// Alert condition met, armed, and outside cooldown -> should fire notification
    Triggered {
        trigger_price: Decimal,
        previous_price: Option<Decimal>,
    },
    /// Price has crossed the hysteresis reset band -> alert is re-armed
    ResetArmed,
    /// Alert condition met but suppressed because cooldown is still active
    SuppressedCooldown {
        trigger_price: Decimal,
        remaining_cooldown: Duration,
    },
    /// No change in alert state
    NoChange,
}

/// Evaluates if an individual condition is satisfied given market data and rolling metrics
pub fn check_condition_met(
    condition: &AlertCondition,
    market_data: &MarketData,
    rolling_stats: Option<&RollingMoveStats>,
    baseline_price: Option<Decimal>,
) -> bool {
    match condition {
        AlertCondition::PriceAbove(target) => market_data.price >= *target,
        AlertCondition::PriceBelow(target) => market_data.price <= *target,
        AlertCondition::PercentageChange(pct) => {
            let base = baseline_price.unwrap_or(Decimal::ZERO);
            let target = base * (Decimal::ONE + (*pct / dec!(100)));
            if *pct >= Decimal::ZERO {
                market_data.price >= target
            } else {
                market_data.price <= target
            }
        }
        AlertCondition::MetricThreshold { metric, op, value } => {
            let actual = match metric {
                MetricTarget::Price => market_data.price,
                MetricTarget::Volume24h => market_data.volume_24hr,
                MetricTarget::QuoteVolume24h => market_data.quote_volume_24hr,
            };
            op.matches(actual, *value)
        }
        AlertCondition::RollingWindowMove { percent, .. } => {
            if let Some(stats) = rolling_stats {
                if *percent >= Decimal::ZERO {
                    stats.net_change_pct.abs() >= *percent || stats.max_swing_pct >= *percent
                } else {
                    stats.net_change_pct <= *percent
                }
            } else {
                false
            }
        }
        AlertCondition::All(items) => {
            items.iter().all(|item| check_condition_met(item, market_data, rolling_stats, baseline_price))
        }
        AlertCondition::Any(items) => {
            items.iter().any(|item| check_condition_met(item, market_data, rolling_stats, baseline_price))
        }
    }
}

/// Extracts rolling window duration from condition if present
pub fn find_window_duration(condition: &AlertCondition) -> Option<Duration> {
    match condition {
        AlertCondition::RollingWindowMove { window_seconds, .. } => {
            Some(Duration::seconds(*window_seconds as i64))
        }
        AlertCondition::All(items) | AlertCondition::Any(items) => {
            for item in items {
                if let Some(dur) = find_window_duration(item) {
                    return Some(dur);
                }
            }
            None
        }
        _ => None,
    }
}

/// Pure function evaluating an alert against current market conditions,
/// supporting composite conditions, rolling windows, cooldowns, and hysteresis reset.
pub fn evaluate_alert(
    alert: &Alert,
    market_data: &MarketData,
    rolling_stats: Option<&RollingMoveStats>,
    previous_price: Option<Decimal>,
    hysteresis_rate: Decimal,
    now: DateTime<Utc>,
) -> AlertEvaluation {
    let current_price = market_data.price;
    let is_met = check_condition_met(
        &alert.condition,
        market_data,
        rolling_stats,
        alert.baseline_price,
    );

    if alert.is_triggered {
        // Alert has already fired. Check if condition cleared and crossed hysteresis reset band
        if !is_met {
            let reset_price = alert.reset_price(hysteresis_rate);
            if !reset_price.is_zero() {
                if alert.is_upward() {
                    if current_price < reset_price {
                        return AlertEvaluation::ResetArmed;
                    }
                } else {
                    if current_price > reset_price {
                        return AlertEvaluation::ResetArmed;
                    }
                }
            } else {
                // Non-price or rolling condition cleared
                return AlertEvaluation::ResetArmed;
            }
        }
        AlertEvaluation::NoChange
    } else {
        // Alert is armed. Check if condition is met
        if is_met {
            // Check cooldown tracker
            if CooldownTracker::is_cooling_down(alert.last_triggered_at, alert.cooldown_seconds, now) {
                let remaining = CooldownTracker::time_remaining(
                    alert.last_triggered_at,
                    alert.cooldown_seconds,
                    now,
                )
                .unwrap_or_else(Duration::zero);

                AlertEvaluation::SuppressedCooldown {
                    trigger_price: current_price,
                    remaining_cooldown: remaining,
                }
            } else {
                AlertEvaluation::Triggered {
                    trigger_price: current_price,
                    previous_price,
                }
            }
        } else {
            AlertEvaluation::NoChange
        }
    }
}

/// Helper for simple price evaluation in tests
pub fn evaluate_price_only(
    alert: &Alert,
    current_price: Decimal,
    previous_price: Option<Decimal>,
    hysteresis_rate: Decimal,
    now: DateTime<Utc>,
) -> AlertEvaluation {
    let dummy = MarketData {
        symbol: alert.symbol.clone(),
        price: current_price,
        price_change_24hr: Decimal::ZERO,
        price_change_percent_24hr: Decimal::ZERO,
        high_price_24hr: current_price,
        low_price_24hr: current_price,
        volume_24hr: Decimal::ZERO,
        quote_volume_24hr: Decimal::ZERO,
        update_at: now,
    };
    evaluate_alert(alert, &dummy, None, previous_price, hysteresis_rate, now)
}

/// Alert Engine service consuming normalized market update events
/// and dispatching notifications through an asynchronous channel.
#[derive(Debug, Clone)]
pub struct AlertEngine {
    store: Arc<AlertStore>,
    market_state: Option<MarketState>,
    notification_tx: mpsc::Sender<AlertNotification>,
    hysteresis_rate: Decimal,
}

impl AlertEngine {
    pub fn new(
        store: Arc<AlertStore>,
        notification_tx: mpsc::Sender<AlertNotification>,
    ) -> Self {
        Self {
            store,
            market_state: None,
            notification_tx,
            hysteresis_rate: DEFAULT_HYSTERESIS_RATE,
        }
    }

    pub fn with_market_state(mut self, market_state: MarketState) -> Self {
        self.market_state = Some(market_state);
        self
    }

    #[allow(dead_code)]
    pub fn with_hysteresis_rate(mut self, rate: Decimal) -> Self {
        self.hysteresis_rate = rate;
        self
    }

    /// Spawns the background alert evaluation loop listening to market events
    pub async fn run(self, mut market_rx: broadcast::Receiver<MarketUpdateEvent>) {
        info!("✓ Alert Engine processing loop started");

        while let Ok(event) = market_rx.recv().await {
            if let MarketUpdateEvent::TickerUpdated { data, previous_price } = event {
                self.process_ticker(&data, previous_price).await;
            }
        }

        warn!("Alert Engine stopped: market broadcast channel closed");
    }

    /// Evaluates all active alerts registered for a given symbol
    pub async fn process_ticker(
        &self,
        market_data: &MarketData,
        previous_price: Option<Decimal>,
    ) {
        let alerts = self.store.get_alerts_for_symbol(&market_data.symbol).await;
        if alerts.is_empty() {
            return;
        }

        let now = Utc::now();

        for alert in alerts {
            // Retrieve rolling window stats if needed by condition
            let rolling_stats = if let Some(dur) = find_window_duration(&alert.condition) {
                if let Some(ref ms) = self.market_state {
                    ms.get_window_stats(&market_data.symbol, dur, now).await
                } else {
                    None
                }
            } else {
                None
            };

            let eval = evaluate_alert(
                &alert,
                market_data,
                rolling_stats.as_ref(),
                previous_price,
                self.hysteresis_rate,
                now,
            );

            match eval {
                AlertEvaluation::Triggered { trigger_price, previous_price } => {
                    // Atomically acquire Redis cooldown lock (prioritizes memory, then SET NX)
                    let acquired = self.store.try_acquire_cooldown(
                        alert.id,
                        alert.cooldown_seconds,
                        alert.last_triggered_at,
                        now,
                    ).await;

                    if !acquired {
                        // Cooldown is active in Redis: hydrate in-memory state to suppress future ticks in RAM
                        self.store
                            .update_trigger_state(alert.id, &alert.symbol, true, Some(now))
                            .await;

                        let remaining = CooldownTracker::time_remaining(
                            alert.last_triggered_at,
                            alert.cooldown_seconds,
                            now,
                        )
                        .unwrap_or_else(|| Duration::seconds(alert.cooldown_seconds as i64));

                        trace!(
                            alert_id = alert.id,
                            symbol = %alert.symbol,
                            price = %trigger_price,
                            remaining_secs = remaining.num_seconds(),
                            "Alert condition met but suppressed by distributed Redis cooldown"
                        );
                        continue;
                    }

                    info!(
                        alert_id = alert.id,
                        symbol = %alert.symbol,
                        price = %trigger_price,
                        user = %alert.user_discord_id,
                        "🔔 Alert triggered! Dispatching notification"
                    );

                    // Update state to triggered with timestamp
                    self.store
                        .update_trigger_state(alert.id, &alert.symbol, true, Some(now))
                        .await;

                    let notification = AlertNotification {
                        alert_id: alert.id,
                        user_discord_id: alert.user_discord_id.clone(),
                        channel_discord_id: alert.channel_discord_id.clone(),
                        symbol: alert.symbol.clone(),
                        condition: alert.condition,
                        trigger_price,
                        previous_price,
                        baseline_price: alert.baseline_price,
                        triggered_at: now,
                    };

                    if let Err(e) = self.notification_tx.send(notification).await {
                        warn!("Failed to dispatch alert notification to channel: {e}");
                    }
                }
                AlertEvaluation::ResetArmed => {
                    debug!(
                        alert_id = alert.id,
                        symbol = %alert.symbol,
                        price = %market_data.price,
                        "↺ Alert re-armed after condition/hysteresis reset"
                    );

                    self.store
                        .update_trigger_state(alert.id, &alert.symbol, false, None)
                        .await;
                }
                AlertEvaluation::SuppressedCooldown { trigger_price, remaining_cooldown } => {
                    trace!(
                        alert_id = alert.id,
                        symbol = %alert.symbol,
                        price = %trigger_price,
                        remaining_secs = remaining_cooldown.num_seconds(),
                        "Alert condition met but suppressed by active cooldown"
                    );
                }
                AlertEvaluation::NoChange => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_alert(
        id: i64,
        condition: AlertCondition,
        baseline_price: Option<Decimal>,
        cooldown_seconds: u32,
        last_triggered_at: Option<DateTime<Utc>>,
        is_triggered: bool,
    ) -> Alert {
        Alert {
            id,
            user_id: 1,
            user_discord_id: "123".to_string(),
            channel_id: 1,
            channel_discord_id: "456".to_string(),
            symbol: "BTCUSDT".to_string(),
            threshold: condition.threshold_value(),
            condition,
            baseline_price,
            cooldown_seconds,
            last_triggered_at,
            is_triggered,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_edge_triggering_and_hysteresis_price_above() {
        let now = Utc::now();
        let alert = make_test_alert(
            1,
            AlertCondition::PriceAbove(dec!(100000)),
            None,
            1800,
            None,
            false,
        );
        let hysteresis = dec!(0.005);

        // 1. Below threshold
        assert_eq!(
            evaluate_price_only(&alert, dec!(98000), None, hysteresis, now),
            AlertEvaluation::NoChange
        );

        // 2. Crosses threshold
        assert_eq!(
            evaluate_price_only(&alert, dec!(100000), Some(dec!(98000)), hysteresis, now),
            AlertEvaluation::Triggered {
                trigger_price: dec!(100000),
                previous_price: Some(dec!(98000))
            }
        );

        // 3. Triggered state -> still above reset band ($99,500)
        let mut triggered = alert.clone();
        triggered.is_triggered = true;
        triggered.last_triggered_at = Some(now);

        assert_eq!(
            evaluate_price_only(&triggered, dec!(99700), Some(dec!(100000)), hysteresis, now),
            AlertEvaluation::NoChange
        );

        // 4. Drops below reset band -> ResetArmed!
        assert_eq!(
            evaluate_price_only(&triggered, dec!(99400), Some(dec!(99700)), hysteresis, now),
            AlertEvaluation::ResetArmed
        );
    }

    #[test]
    fn test_multi_condition_and_triggering() {
        let now = Utc::now();
        let cond = AlertCondition::All(vec![
            AlertCondition::PriceAbove(dec!(100000)),
            AlertCondition::MetricThreshold {
                metric: MetricTarget::Volume24h,
                op: ComparisonOp::GreaterThan,
                value: dec!(50000000000),
            },
        ]);
        let alert = make_test_alert(2, cond, None, 1800, None, false);

        // 1. Price is met ($105,000) but volume is NOT met ($40B) -> NoChange
        let data1 = MarketData {
            symbol: "BTCUSDT".to_string(),
            price: dec!(105000),
            price_change_24hr: dec!(2000),
            price_change_percent_24hr: dec!(2.0),
            high_price_24hr: dec!(106000),
            low_price_24hr: dec!(100000),
            volume_24hr: dec!(40000000000),
            quote_volume_24hr: dec!(4000000000000),
            update_at: now,
        };
        assert_eq!(
            evaluate_alert(&alert, &data1, None, None, dec!(0.005), now),
            AlertEvaluation::NoChange
        );

        // 2. Both Price ($105,000) and Volume ($55B) are met -> Triggered!
        let mut data2 = data1.clone();
        data2.volume_24hr = dec!(55000000000);
        assert_eq!(
            evaluate_alert(&alert, &data2, None, Some(dec!(104000)), dec!(0.005), now),
            AlertEvaluation::Triggered {
                trigger_price: dec!(105000),
                previous_price: Some(dec!(104000))
            }
        );
    }

    #[test]
    fn test_rolling_window_volatility_triggering() {
        let now = Utc::now();
        let cond = AlertCondition::RollingWindowMove {
            percent: dec!(3),
            window_seconds: 300,
        };
        let alert = make_test_alert(3, cond, None, 1800, None, false);

        let data = MarketData {
            symbol: "BTCUSDT".to_string(),
            price: dec!(65000),
            price_change_24hr: dec!(1000),
            price_change_percent_24hr: dec!(1.5),
            high_price_24hr: dec!(66000),
            low_price_24hr: dec!(64000),
            volume_24hr: dec!(10000),
            quote_volume_24hr: dec!(650000000),
            update_at: now,
        };

        // 1. Move in window is only 1.2% -> NoChange
        let stats_low = RollingMoveStats {
            start_price: dec!(64200),
            current_price: dec!(65000),
            min_price: dec!(64200),
            max_price: dec!(65000),
            net_change_pct: dec!(1.24),
            max_swing_pct: dec!(1.24),
            sample_count: 30,
        };
        assert_eq!(
            evaluate_alert(&alert, &data, Some(&stats_low), None, dec!(0.005), now),
            AlertEvaluation::NoChange
        );

        // 2. Move in window is 3.5% -> Triggered!
        let stats_high = RollingMoveStats {
            start_price: dec!(62800),
            current_price: dec!(65000),
            min_price: dec!(62800),
            max_price: dec!(65000),
            net_change_pct: dec!(3.5),
            max_swing_pct: dec!(3.5),
            sample_count: 30,
        };
        assert_eq!(
            evaluate_alert(&alert, &data, Some(&stats_high), None, dec!(0.005), now),
            AlertEvaluation::Triggered {
                trigger_price: dec!(65000),
                previous_price: None
            }
        );
    }
}
