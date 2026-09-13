use std::sync::Arc;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, trace, warn};

use crate::alerts::cooldown::CooldownTracker;
use crate::alerts::models::{Alert, AlertCondition, DEFAULT_HYSTERESIS_RATE};
use crate::alerts::store::AlertStore;
use crate::market::MarketUpdateEvent;

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

/// Pure function evaluating an alert against current market conditions
pub fn evaluate_alert(
    alert: &Alert,
    current_price: Decimal,
    previous_price: Option<Decimal>,
    hysteresis_rate: Decimal,
    now: DateTime<Utc>,
) -> AlertEvaluation {
    let target = alert.target_price();
    let reset_price = alert.reset_price(hysteresis_rate);
    let is_upward = alert.is_upward();

    if is_upward {
        // --- UPWARD ALERT (e.g. PriceAbove $100,000, or PercentageChange +5%) ---
        if alert.is_triggered {
            // Alert has already fired. Must cross BELOW reset band ($99,500) to re-arm.
            if current_price < reset_price {
                AlertEvaluation::ResetArmed
            } else {
                AlertEvaluation::NoChange
            }
        } else {
            // Alert is ARMED. Check if price crossed target.
            if current_price >= target {
                // Check cooldown
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
    } else {
        // --- DOWNWARD ALERT (e.g. PriceBelow $4,000, or PercentageChange -5%) ---
        if alert.is_triggered {
            // Alert has already fired. Must cross ABOVE reset band ($4,020) to re-arm.
            if current_price > reset_price {
                AlertEvaluation::ResetArmed
            } else {
                AlertEvaluation::NoChange
            }
        } else {
            // Alert is ARMED. Check if price crossed target.
            if current_price <= target {
                // Check cooldown
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
}

/// Alert Engine service consuming normalized market update events
/// and dispatching notifications through an asynchronous channel.
#[derive(Debug, Clone)]
pub struct AlertEngine {
    store: Arc<AlertStore>,
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
            notification_tx,
            hysteresis_rate: DEFAULT_HYSTERESIS_RATE,
        }
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
                self.process_ticker(data.symbol, data.price, previous_price).await;
            }
        }

        warn!("Alert Engine stopped: market broadcast channel closed");
    }

    /// Evaluates all active alerts registered for a given symbol
    pub async fn process_ticker(
        &self,
        symbol: String,
        current_price: Decimal,
        previous_price: Option<Decimal>,
    ) {
        let alerts = self.store.get_alerts_for_symbol(&symbol).await;
        if alerts.is_empty() {
            return;
        }

        let now = Utc::now();

        for alert in alerts {
            // Check distributed Redis cooldown state
            let is_cooling = self.store.is_cooling_down(
                alert.id,
                alert.last_triggered_at,
                alert.cooldown_seconds,
                now,
            ).await;

            // If Redis indicates cooling down but in-memory last_triggered_at is missing, hydrate it
            let mut alert_eval_copy = alert.clone();
            if is_cooling && alert_eval_copy.last_triggered_at.is_none() {
                alert_eval_copy.last_triggered_at = Some(now);
            }

            let eval = evaluate_alert(
                &alert_eval_copy,
                current_price,
                previous_price,
                self.hysteresis_rate,
                now,
            );

            match eval {
                AlertEvaluation::Triggered { trigger_price, previous_price } => {
                    // Atomically acquire Redis cooldown lock: SET cooldown:{alert_id} 1 EX {secs} NX
                    let acquired = self.store.try_acquire_cooldown(
                        alert.id,
                        alert.cooldown_seconds,
                        alert.last_triggered_at,
                        now,
                    ).await;

                    if !acquired {
                        let remaining = self.store.get_cooldown_remaining(
                            alert.id,
                            alert.last_triggered_at,
                            alert.cooldown_seconds,
                            now,
                        ).await.unwrap_or_else(Duration::zero);

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
                        price = %current_price,
                        "↺ Alert re-armed after crossing hysteresis reset band"
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
    use rust_decimal_macros::dec;

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
        // Threshold: $100,000, 0.5% hysteresis -> reset at $99,500
        let alert = make_test_alert(
            1,
            AlertCondition::PriceAbove(dec!(100000)),
            None,
            1800,
            None,
            false, // Armed
        );

        let hysteresis = dec!(0.005);

        // 1. Price is below threshold ($98,000) -> NoChange
        let res = evaluate_alert(&alert, dec!(98000), None, hysteresis, now);
        assert_eq!(res, AlertEvaluation::NoChange);

        // 2. Price crosses threshold ($100,000) -> Triggered!
        let res = evaluate_alert(&alert, dec!(100000), Some(dec!(98000)), hysteresis, now);
        assert_eq!(
            res,
            AlertEvaluation::Triggered {
                trigger_price: dec!(100000),
                previous_price: Some(dec!(98000))
            }
        );

        // 3. Now alert is marked as triggered in state
        let mut triggered_alert = alert.clone();
        triggered_alert.is_triggered = true;
        triggered_alert.last_triggered_at = Some(now);

        // 4. Price continues rising ($101,000) -> NoChange (no duplicate notification!)
        let res = evaluate_alert(&triggered_alert, dec!(101000), Some(dec!(100000)), hysteresis, now);
        assert_eq!(res, AlertEvaluation::NoChange);

        // 5. Price drops slightly to $99,700 (still above reset band $99,500) -> NoChange
        let res = evaluate_alert(&triggered_alert, dec!(99700), Some(dec!(101000)), hysteresis, now);
        assert_eq!(res, AlertEvaluation::NoChange);

        // 6. Price drops below reset band to $99,400 -> ResetArmed!
        let res = evaluate_alert(&triggered_alert, dec!(99400), Some(dec!(99700)), hysteresis, now);
        assert_eq!(res, AlertEvaluation::ResetArmed);

        // 7. Alert is re-armed
        let mut rearmed_alert = triggered_alert.clone();
        rearmed_alert.is_triggered = false;

        // 8. If price rises to $100,050 while still within cooldown -> SuppressedCooldown
        let res = evaluate_alert(&rearmed_alert, dec!(100050), Some(dec!(99400)), hysteresis, now);
        assert!(matches!(res, AlertEvaluation::SuppressedCooldown { .. }));

        // 9. If price rises to $100,050 after cooldown expires (31 mins later) -> Triggered!
        let future = now + Duration::minutes(31);
        let res = evaluate_alert(&rearmed_alert, dec!(100050), Some(dec!(99400)), hysteresis, future);
        assert_eq!(
            res,
            AlertEvaluation::Triggered {
                trigger_price: dec!(100050),
                previous_price: Some(dec!(99400))
            }
        );
    }

    #[test]
    fn test_edge_triggering_and_hysteresis_price_below() {
        let now = Utc::now();
        // Threshold: $4,000, 0.5% hysteresis -> reset at $4,020
        let alert = make_test_alert(
            2,
            AlertCondition::PriceBelow(dec!(4000)),
            None,
            1800,
            None,
            false, // Armed
        );

        let hysteresis = dec!(0.005);

        // 1. Price is above threshold ($4,100) -> NoChange
        let res = evaluate_alert(&alert, dec!(4100), None, hysteresis, now);
        assert_eq!(res, AlertEvaluation::NoChange);

        // 2. Price crosses below threshold ($3,990) -> Triggered!
        let res = evaluate_alert(&alert, dec!(3990), Some(dec!(4100)), hysteresis, now);
        assert_eq!(
            res,
            AlertEvaluation::Triggered {
                trigger_price: dec!(3990),
                previous_price: Some(dec!(4100))
            }
        );

        // 3. Mark triggered
        let mut triggered_alert = alert.clone();
        triggered_alert.is_triggered = true;
        triggered_alert.last_triggered_at = Some(now);

        // 4. Price bounces slightly to $4,010 (below reset $4,020) -> NoChange
        let res = evaluate_alert(&triggered_alert, dec!(4010), Some(dec!(3990)), hysteresis, now);
        assert_eq!(res, AlertEvaluation::NoChange);

        // 5. Price rises above reset band to $4,025 -> ResetArmed!
        let res = evaluate_alert(&triggered_alert, dec!(4025), Some(dec!(4010)), hysteresis, now);
        assert_eq!(res, AlertEvaluation::ResetArmed);
    }

    #[test]
    fn test_percentage_change_alert_upward() {
        let now = Utc::now();
        // Baseline: $60,000, Condition: +5% -> Target: $63,000, Reset at $63,000 * 0.995 = $62,685
        let alert = make_test_alert(
            3,
            AlertCondition::PercentageChange(dec!(5)),
            Some(dec!(60000)),
            1800,
            None,
            false,
        );

        let hysteresis = dec!(0.005);

        // Price at $62,000 -> NoChange
        assert_eq!(
            evaluate_alert(&alert, dec!(62000), None, hysteresis, now),
            AlertEvaluation::NoChange
        );

        // Price hits $63,000 -> Triggered!
        assert_eq!(
            evaluate_alert(&alert, dec!(63000), Some(dec!(62000)), hysteresis, now),
            AlertEvaluation::Triggered {
                trigger_price: dec!(63000),
                previous_price: Some(dec!(62000))
            }
        );
    }
}
