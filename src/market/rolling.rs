use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::RwLock;

/// Individual tick measurement recorded in the rolling buffer
#[derive(Debug, Clone, PartialEq)]
pub struct PriceTick {
    pub timestamp: DateTime<Utc>,
    pub price: Decimal,
}

/// Aggregated metrics for a given rolling time window
#[derive(Debug, Clone, PartialEq)]
pub struct RollingMoveStats {
    /// Earliest price recorded inside the evaluated window
    pub start_price: Decimal,
    /// Most recent price recorded inside the window
    pub current_price: Decimal,
    /// Lowest price recorded during the window
    pub min_price: Decimal,
    /// Highest price recorded during the window
    pub max_price: Decimal,
    /// Net percentage change: ((current - start) / start) * 100
    pub net_change_pct: Decimal,
    /// Maximum peak-to-trough swing percentage: ((max - min) / min) * 100
    pub max_swing_pct: Decimal,
    /// Total number of sample ticks considered in the calculation
    pub sample_count: usize,
}

/// In-memory ring buffer of price ticks with automatic retention eviction
#[derive(Debug, Clone)]
pub struct RollingPriceBuffer {
    buffer: VecDeque<PriceTick>,
    max_retention: Duration,
    max_capacity: usize,
}

impl RollingPriceBuffer {
    pub fn new(max_retention: Duration, max_capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(max_capacity.min(4096)),
            max_retention,
            max_capacity,
        }
    }

    /// Appends a new price tick and evicts expired or overflow ticks
    pub fn record_tick(&mut self, timestamp: DateTime<Utc>, price: Decimal) {
        // Enforce chronological insertion (handle rare out-of-order ticks)
        if let Some(last) = self.buffer.back() {
            if timestamp < last.timestamp {
                return;
            }
        }

        self.buffer.push_back(PriceTick { timestamp, price });

        // Evict expired ticks
        let cutoff = timestamp - self.max_retention;
        while let Some(front) = self.buffer.front() {
            if front.timestamp < cutoff || self.buffer.len() > self.max_capacity {
                self.buffer.pop_front();
            } else {
                break;
            }
        }
    }

    /// Evaluates price movements over the specified duration relative to `now`
    pub fn calculate_window_move(
        &self,
        now: DateTime<Utc>,
        window_duration: Duration,
    ) -> Option<RollingMoveStats> {
        if self.buffer.is_empty() {
            return None;
        }

        let window_start = now - window_duration;

        // Collect all ticks within the window [window_start, now]
        let mut min_price = None;
        let mut max_price = None;
        let mut start_price = None;
        let mut latest_price = None;
        let mut count = 0;

        for tick in self.buffer.iter() {
            if tick.timestamp >= window_start && tick.timestamp <= now {
                if start_price.is_none() {
                    start_price = Some(tick.price);
                }
                latest_price = Some(tick.price);

                min_price = Some(match min_price {
                    Some(cur_min) if cur_min < tick.price => cur_min,
                    _ => tick.price,
                });

                max_price = Some(match max_price {
                    Some(cur_max) if cur_max > tick.price => cur_max,
                    _ => tick.price,
                });

                count += 1;
            }
        }

        // If no ticks fall strictly within the window, use the latest known tick if available
        let start = start_price.or_else(|| self.buffer.back().map(|t| t.price))?;
        let current = latest_price.or_else(|| self.buffer.back().map(|t| t.price))?;
        let min = min_price.unwrap_or(current);
        let max = max_price.unwrap_or(current);

        let net_change_pct = if start.is_zero() {
            Decimal::ZERO
        } else {
            ((current - start) / start) * dec!(100)
        };

        let max_swing_pct = if min.is_zero() {
            Decimal::ZERO
        } else {
            ((max - min) / min) * dec!(100)
        };

        Some(RollingMoveStats {
            start_price: start,
            current_price: current,
            min_price: min,
            max_price: max,
            net_change_pct,
            max_swing_pct,
            sample_count: count,
        })
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

/// Thread-safe tracker maintaining rolling price buffers across symbols
#[derive(Debug, Clone)]
pub struct RollingWindowTracker {
    buffers: Arc<RwLock<HashMap<String, RollingPriceBuffer>>>,
    max_retention: Duration,
    max_capacity_per_symbol: usize,
}

impl Default for RollingWindowTracker {
    fn default() -> Self {
        Self::new(Duration::hours(2), 7200)
    }
}

impl RollingWindowTracker {
    pub fn new(max_retention: Duration, max_capacity_per_symbol: usize) -> Self {
        Self {
            buffers: Arc::new(RwLock::new(HashMap::new())),
            max_retention,
            max_capacity_per_symbol,
        }
    }

    /// Records a new tick for the given trading pair symbol
    pub async fn record_tick(&self, symbol: &str, timestamp: DateTime<Utc>, price: Decimal) {
        let key = symbol.to_uppercase();
        let mut map = self.buffers.write().await;
        let buf = map
            .entry(key)
            .or_insert_with(|| RollingPriceBuffer::new(self.max_retention, self.max_capacity_per_symbol));
        buf.record_tick(timestamp, price);
    }

    /// Retrieves rolling move metrics for a symbol over the specified duration
    pub async fn get_window_stats(
        &self,
        symbol: &str,
        window_duration: Duration,
        now: DateTime<Utc>,
    ) -> Option<RollingMoveStats> {
        let key = symbol.to_uppercase();
        let map = self.buffers.read().await;
        let buf = map.get(&key)?;
        buf.calculate_window_move(now, window_duration)
    }

    /// Clears expired symbols or resets state
    #[allow(dead_code)]
    pub async fn clear(&self) {
        let mut map = self.buffers.write().await;
        map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_base_time() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    #[test]
    fn test_flat_market_calculation() {
        let base_time = generate_base_time();
        let mut buffer = RollingPriceBuffer::new(Duration::minutes(60), 3600);

        // 60 ticks over 10 minutes, price steady at $50,000
        for i in 0..60 {
            let tick_time = base_time + Duration::seconds(i * 10);
            buffer.record_tick(tick_time, dec!(50000));
        }

        let now = base_time + Duration::seconds(600);
        let stats = buffer
            .calculate_window_move(now, Duration::minutes(5))
            .expect("Expected stats");

        assert_eq!(stats.start_price, dec!(50000));
        assert_eq!(stats.current_price, dec!(50000));
        assert_eq!(stats.min_price, dec!(50000));
        assert_eq!(stats.max_price, dec!(50000));
        assert_eq!(stats.net_change_pct, dec!(0));
        assert_eq!(stats.max_swing_pct, dec!(0));
    }

    #[test]
    fn test_sudden_spike_volatility() {
        let base_time = generate_base_time();
        let mut buffer = RollingPriceBuffer::new(Duration::minutes(60), 3600);

        // 0 to 2 minutes: steady at $50,000
        for i in 0..12 {
            let tick_time = base_time + Duration::seconds(i * 10);
            buffer.record_tick(tick_time, dec!(50000));
        }

        // At 3 minutes (180s): price jumps to $52,000 (+4%)
        let spike_time = base_time + Duration::seconds(180);
        buffer.record_tick(spike_time, dec!(52000));

        let now = spike_time;
        let stats = buffer
            .calculate_window_move(now, Duration::minutes(5))
            .expect("Expected stats");

        assert_eq!(stats.start_price, dec!(50000));
        assert_eq!(stats.current_price, dec!(52000));
        assert_eq!(stats.min_price, dec!(50000));
        assert_eq!(stats.max_price, dec!(52000));
        assert_eq!(stats.net_change_pct, dec!(4));
        assert_eq!(stats.max_swing_pct, dec!(4));
    }

    #[test]
    fn test_window_expiration_drops_old_jump() {
        let base_time = generate_base_time();
        let mut buffer = RollingPriceBuffer::new(Duration::minutes(60), 3600);

        // At T=0: $50,000
        buffer.record_tick(base_time, dec!(50000));

        // At T=60s: jumps to $52,000 (+4%)
        buffer.record_tick(base_time + Duration::seconds(60), dec!(52000));

        // Stays at $52,000 up to T=400s (over 6 minutes later)
        for s in (120..=400).step_by(30) {
            buffer.record_tick(base_time + Duration::seconds(s), dec!(52000));
        }

        // Window of 5 minutes evaluated at T=400s:
        // Window is [400 - 300 = 100s, 400s].
        // T=0 and T=60s are outside this window!
        let now = base_time + Duration::seconds(400);
        let stats = buffer
            .calculate_window_move(now, Duration::minutes(5))
            .expect("Expected stats");

        // Earliest in window is at T=120s which was $52,000
        assert_eq!(stats.start_price, dec!(52000));
        assert_eq!(stats.current_price, dec!(52000));
        assert_eq!(stats.net_change_pct, dec!(0));
        assert_eq!(stats.max_swing_pct, dec!(0));
    }

    #[test]
    fn test_oscillation_peak_trough_whipsaw() {
        let base_time = generate_base_time();
        let mut buffer = RollingPriceBuffer::new(Duration::minutes(60), 3600);

        // T=0s: $100
        buffer.record_tick(base_time, dec!(100));
        // T=60s: drops to $95 (-5%)
        buffer.record_tick(base_time + Duration::seconds(60), dec!(95));
        // T=120s: spikes to $105 (+5% from start, +10.52% from trough)
        buffer.record_tick(base_time + Duration::seconds(120), dec!(105));
        // T=180s: closes at $102
        buffer.record_tick(base_time + Duration::seconds(180), dec!(102));

        let now = base_time + Duration::seconds(180);
        let stats = buffer
            .calculate_window_move(now, Duration::minutes(5))
            .expect("Expected stats");

        assert_eq!(stats.start_price, dec!(100));
        assert_eq!(stats.current_price, dec!(102));
        assert_eq!(stats.min_price, dec!(95));
        assert_eq!(stats.max_price, dec!(105));
        assert_eq!(stats.net_change_pct, dec!(2)); // (102 - 100) / 100 * 100 = +2%
        // Max swing: (105 - 95) / 95 * 100 = 10 / 95 * 100 = 10.52631578947368421052631579%
        assert!(stats.max_swing_pct > dec!(10.52) && stats.max_swing_pct < dec!(10.53));
    }

    #[test]
    fn test_capacity_and_retention_eviction() {
        let base_time = generate_base_time();
        // Limit capacity to 50 ticks, max retention 10 minutes
        let mut buffer = RollingPriceBuffer::new(Duration::minutes(10), 50);

        // Insert 200 ticks spaced 10 seconds apart (2000s = 33 minutes)
        for i in 0..200 {
            let t = base_time + Duration::seconds(i * 10);
            buffer.record_tick(t, dec!(100) + Decimal::from(i));
        }

        assert!(buffer.len() <= 50, "Buffer capacity should not exceed 50");
        assert!(buffer.len() > 0);

        // Ensure the oldest tick in buffer is relatively recent
        let latest_time = base_time + Duration::seconds(1990);
        let cutoff = latest_time - Duration::minutes(10);
        assert!(buffer.buffer.front().unwrap().timestamp >= cutoff);
    }
}
