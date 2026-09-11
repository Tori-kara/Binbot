use std::time::Duration;

pub const DEFAULT_BINANCE_WS_RAW_URL: &str = "wss://stream.binance.com:9443/ws";
pub const DEFAULT_BINANCE_WS_COMBINED_URL: &str = "wss://stream.binance.com:9443/stream";

/// Configuration options for Binance WebSocket client
#[derive(Debug, Clone)]
pub struct BinanceWsConfig {
    pub base_url: String,
    pub initial_reconnect_delay: Duration,
    pub max_reconnect_delay: Duration,
    pub event_buffer_size: usize,
}

impl BinanceWsConfig {
    /// Creates a configuration with a custom base WebSocket URL
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            ..Default::default()
        }
    }
}

impl Default for BinanceWsConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BINANCE_WS_COMBINED_URL.to_string(),
            initial_reconnect_delay: Duration::from_secs(1),
            max_reconnect_delay: Duration::from_secs(60),
            event_buffer_size: 2048,
        }
    }
}

/// Helper methods to generate standardized Binance stream names (case-insensitive conversion to lowercase)
pub fn ticker_stream(symbol: &str) -> String {
    format!("{}@ticker", symbol.to_ascii_lowercase())
}

pub fn mini_ticker_stream(symbol: &str) -> String {
    format!("{}@miniTicker", symbol.to_ascii_lowercase())
}

pub fn trade_stream(symbol: &str) -> String {
    format!("{}@trade", symbol.to_ascii_lowercase())
}

pub fn agg_trade_stream(symbol: &str) -> String {
    format!("{}@aggTrade", symbol.to_ascii_lowercase())
}

pub fn kline_stream(symbol: &str, interval: &str) -> String {
    format!("{}@kline_{}", symbol.to_ascii_lowercase(), interval)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_helpers() {
        assert_eq!(ticker_stream("BTCUSDT"), "btcusdt@ticker");
        assert_eq!(mini_ticker_stream("ethusdt"), "ethusdt@miniTicker");
        assert_eq!(trade_stream("SOLUSDT"), "solusdt@trade");
        assert_eq!(agg_trade_stream("BNBUSDT"), "bnbusdt@aggTrade");
        assert_eq!(kline_stream("BTCUSDT", "1m"), "btcusdt@kline_1m");
    }
}
