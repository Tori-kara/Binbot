use std::collections::HashMap;
use std::sync::Arc;
use rust_decimal::Decimal;
use tokio::sync::RwLock;

use crate::market::models::MarketData;

/// Thread-safe in-memory cache for market data
#[derive(Debug, Clone, Default)]
pub struct MarketState {
    data: Arc<RwLock<HashMap<String, MarketData>>>,
}

impl MarketState {
    /// Creates a new empty `MarketState`
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Updates or inserts market data for a symbol.
    /// Returns the previous price if the symbol was already tracked.
    pub async fn update(&self, data: MarketData) -> Option<Decimal> {
        let symbol = data.symbol.to_uppercase();
        let mut map = self.data.write().await;
        let prev_price = map.get(&symbol).map(|prev| prev.price);
        map.insert(symbol, data);
        prev_price
    }

    /// Retrieves the current price for a symbol (case-insensitive)
    pub async fn get_price(&self, symbol: &str) -> Option<Decimal> {
        let key = symbol.to_uppercase();
        let map = self.data.read().await;
        map.get(&key).map(|d| d.price)
    }

    /// Retrieves a cloned snapshot of the market data for a symbol (case-insensitive)
    pub async fn get_snapshot(&self, symbol: &str) -> Option<MarketData> {
        let key = symbol.to_uppercase();
        let map = self.data.read().await;
        map.get(&key).cloned()
    }

    /// Retrieves a list of all currently tracked symbols (for Discord autocomplete)
    pub async fn get_symbols(&self) -> Vec<String> {
        let map = self.data.read().await;
        map.keys().cloned().collect()
    }

    /// Retrieves snapshots of all tracked symbols
    pub async fn get_all_snapshots(&self) -> Vec<MarketData> {
        let map = self.data.read().await;
        map.values().cloned().collect()
    }

    /// Returns the total number of tracked symbols
    pub async fn count(&self) -> usize {
        let map = self.data.read().await;
        map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_market_state_operations() {
        let state = MarketState::new();

        let btc_data = MarketData {
            symbol: "BTCUSDT".to_string(),
            price: dec!(65000.00),
            price_change_24hr: dec!(1500.00),
            price_change_percent_24hr: dec!(2.35),
            high_price_24hr: dec!(66000.00),
            low_price_24hr: dec!(63500.00),
            volume_24hr: dec!(10000.0),
            quote_volume_24hr: dec!(650000000.0),
            update_at: Utc::now(),
        };

        // First update: previous price is None
        let prev = state.update(btc_data.clone()).await;
        assert_eq!(prev, None);

        // Case-insensitive query
        let price = state.get_price("btcusdt").await;
        assert_eq!(price, Some(dec!(65000.00)));

        // Second update: previous price is returned
        let mut updated_btc = btc_data.clone();
        updated_btc.price = dec!(65500.00);
        let prev = state.update(updated_btc).await;
        assert_eq!(prev, Some(dec!(65000.00)));

        assert_eq!(state.get_price("BTCUSDT").await, Some(dec!(65500.00)));
        assert_eq!(state.count().await, 1);
        assert_eq!(state.get_symbols().await, vec!["BTCUSDT".to_string()]);
    }
}
