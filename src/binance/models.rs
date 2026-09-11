use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Generic envelope for Binance combined streams:
/// `{"stream": "<streamName>", "data": <rawPayload>}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinedStreamPayload<T> {
    pub stream: String,
    pub data: T,
}

/// Dynamic subscription request payload sent to Binance WebSocket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRequest {
    pub method: String,
    pub params: Vec<String>,
    pub id: u64,
}

impl SubscriptionRequest {
    pub fn subscribe(streams: Vec<String>, id: u64) -> Self {
        Self {
            method: "SUBSCRIBE".to_string(),
            params: streams,
            id,
        }
    }

    pub fn unsubscribe(streams: Vec<String>, id: u64) -> Self {
        Self {
            method: "UNSUBSCRIBE".to_string(),
            params: streams,
            id,
        }
    }

    pub fn list(id: u64) -> Self {
        Self {
            method: "LIST_SUBSCRIPTIONS".to_string(),
            params: Vec::new(),
            id,
        }
    }
}

/// Response returned from Binance for subscription management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionResponse {
    pub id: u64,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<SubscriptionError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionError {
    pub code: i64,
    pub msg: String,
}

/// 24-hour Rolling Window Ticker Event (<symbol>@ticker)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerEvent {
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "p")]
    pub price_change: Decimal,
    #[serde(rename = "P")]
    pub price_change_percent: Decimal,
    #[serde(rename = "w")]
    pub weighted_avg_price: Decimal,
    #[serde(rename = "x")]
    pub prev_close_price: Decimal,
    #[serde(rename = "c")]
    pub current_close: Decimal,
    #[serde(rename = "Q")]
    pub last_quantity: Decimal,
    #[serde(rename = "b")]
    pub best_bid_price: Decimal,
    #[serde(rename = "B")]
    pub best_bid_quantity: Decimal,
    #[serde(rename = "a")]
    pub best_ask_price: Decimal,
    #[serde(rename = "A")]
    pub best_ask_quantity: Decimal,
    #[serde(rename = "o")]
    pub open_price: Decimal,
    #[serde(rename = "h")]
    pub high_price: Decimal,
    #[serde(rename = "l")]
    pub low_price: Decimal,
    #[serde(rename = "v")]
    pub total_base_volume: Decimal,
    #[serde(rename = "q")]
    pub total_quote_volume: Decimal,
    #[serde(rename = "O")]
    pub stat_open_time: u64,
    #[serde(rename = "C")]
    pub stat_close_time: u64,
    #[serde(rename = "F")]
    pub first_trade_id: i64,
    #[serde(rename = "L")]
    pub last_trade_id: i64,
    #[serde(rename = "n")]
    pub total_trades: u64,
}

/// 24-hour Rolling Window Mini-Ticker Event (<symbol>@miniTicker)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniTickerEvent {
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "c")]
    pub current_close: Decimal,
    #[serde(rename = "o")]
    pub open_price: Decimal,
    #[serde(rename = "h")]
    pub high_price: Decimal,
    #[serde(rename = "l")]
    pub low_price: Decimal,
    #[serde(rename = "v")]
    pub total_base_volume: Decimal,
    #[serde(rename = "q")]
    pub total_quote_volume: Decimal,
}

/// Individual Trade Event (<symbol>@trade)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeEvent {
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "t")]
    pub trade_id: u64,
    #[serde(rename = "p")]
    pub price: Decimal,
    #[serde(rename = "q")]
    pub quantity: Decimal,
    #[serde(rename = "b")]
    pub buyer_order_id: Option<u64>,
    #[serde(rename = "a")]
    pub seller_order_id: Option<u64>,
    #[serde(rename = "T")]
    pub trade_time: u64,
    #[serde(rename = "m")]
    pub is_buyer_market_maker: bool,
}

/// Aggregate Trade Event (<symbol>@aggTrade)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggTradeEvent {
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "a")]
    pub agg_trade_id: u64,
    #[serde(rename = "p")]
    pub price: Decimal,
    #[serde(rename = "q")]
    pub quantity: Decimal,
    #[serde(rename = "f")]
    pub first_trade_id: u64,
    #[serde(rename = "l")]
    pub last_trade_id: u64,
    #[serde(rename = "T")]
    pub trade_time: u64,
    #[serde(rename = "m")]
    pub is_buyer_market_maker: bool,
}

/// Candlestick/Kline details inside KlineEvent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KlineData {
    #[serde(rename = "t")]
    pub start_time: u64,
    #[serde(rename = "T")]
    pub end_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "i")]
    pub interval: String,
    #[serde(rename = "f")]
    pub first_trade_id: i64,
    #[serde(rename = "L")]
    pub last_trade_id: i64,
    #[serde(rename = "o")]
    pub open_price: Decimal,
    #[serde(rename = "c")]
    pub close_price: Decimal,
    #[serde(rename = "h")]
    pub high_price: Decimal,
    #[serde(rename = "l")]
    pub low_price: Decimal,
    #[serde(rename = "v")]
    pub base_volume: Decimal,
    #[serde(rename = "n")]
    pub trade_count: u64,
    #[serde(rename = "x")]
    pub is_closed: bool,
    #[serde(rename = "q")]
    pub quote_volume: Decimal,
    #[serde(rename = "V")]
    pub taker_buy_base_volume: Decimal,
    #[serde(rename = "Q")]
    pub taker_buy_quote_volume: Decimal,
}

/// Candlestick/Kline Stream Event (<symbol>@kline_<interval>)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KlineEvent {
    #[serde(rename = "E")]
    pub event_time: u64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "k")]
    pub kline: KlineData,
}

/// Strongly-typed discriminated enum of Binance WebSocket Events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "e")]
pub enum BinanceEvent {
    #[serde(rename = "24hrTicker")]
    Ticker(TickerEvent),
    #[serde(rename = "24hrMiniTicker")]
    MiniTicker(MiniTickerEvent),
    #[serde(rename = "trade")]
    Trade(TradeEvent),
    #[serde(rename = "aggTrade")]
    AggTrade(AggTradeEvent),
    #[serde(rename = "kline")]
    Kline(KlineEvent),
}

impl BinanceEvent {
    /// Returns the symbol associated with this event
    pub fn symbol(&self) -> &str {
        match self {
            BinanceEvent::Ticker(e) => &e.symbol,
            BinanceEvent::MiniTicker(e) => &e.symbol,
            BinanceEvent::Trade(e) => &e.symbol,
            BinanceEvent::AggTrade(e) => &e.symbol,
            BinanceEvent::Kline(e) => &e.symbol,
        }
    }

    /// Returns the latest known price for this event
    pub fn price(&self) -> Decimal {
        match self {
            BinanceEvent::Ticker(e) => e.current_close,
            BinanceEvent::MiniTicker(e) => e.current_close,
            BinanceEvent::Trade(e) => e.price,
            BinanceEvent::AggTrade(e) => e.price,
            BinanceEvent::Kline(e) => e.kline.close_price,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_deserialize_ticker_event() {
        let json = r#"{
            "e": "24hrTicker",
            "E": 123456789,
            "s": "BTCUSDT",
            "p": "150.00",
            "P": "0.25",
            "w": "60000.00",
            "x": "59850.00",
            "c": "60000.00",
            "Q": "0.5",
            "b": "59999.00",
            "B": "1.2",
            "a": "60001.00",
            "A": "2.5",
            "o": "59850.00",
            "h": "60500.00",
            "l": "59500.00",
            "v": "1000.50",
            "q": "60030000.00",
            "O": 123400000,
            "C": 123486400,
            "F": 1000,
            "L": 2000,
            "n": 1001
        }"#;

        let event: BinanceEvent = serde_json::from_str(json).expect("should deserialize ticker");
        if let BinanceEvent::Ticker(ticker) = event {
            assert_eq!(ticker.symbol, "BTCUSDT");
            assert_eq!(ticker.current_close, dec!(60000.00));
            assert_eq!(ticker.high_price, dec!(60500.00));
            assert_eq!(ticker.total_trades, 1001);
        } else {
            panic!("expected Ticker variant");
        }
    }

    #[test]
    fn test_deserialize_combined_stream_event() {
        let json = r#"{
            "stream": "btcusdt@ticker",
            "data": {
                "e": "24hrTicker",
                "E": 123456789,
                "s": "BTCUSDT",
                "p": "150.00",
                "P": "0.25",
                "w": "60000.00",
                "x": "59850.00",
                "c": "60000.00",
                "Q": "0.5",
                "b": "59999.00",
                "B": "1.2",
                "a": "60001.00",
                "A": "2.5",
                "o": "59850.00",
                "h": "60500.00",
                "l": "59500.00",
                "v": "1000.50",
                "q": "60030000.00",
                "O": 123400000,
                "C": 123486400,
                "F": 1000,
                "L": 2000,
                "n": 1001
            }
        }"#;

        let combined: CombinedStreamPayload<BinanceEvent> =
            serde_json::from_str(json).expect("should deserialize combined payload");
        assert_eq!(combined.stream, "btcusdt@ticker");
        assert_eq!(combined.data.symbol(), "BTCUSDT");
        assert_eq!(combined.data.price(), dec!(60000.00));
    }
}
