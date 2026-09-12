use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::binance::TickerEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketData {
    pub symbol: String,
    pub price: Decimal,
    pub price_change_24hr: Decimal,
    pub price_change_percent_24hr: Decimal,
    pub high_price_24hr: Decimal,
    pub low_price_24hr: Decimal,
    pub volume_24hr: Decimal,
    pub quote_volume_24hr: Decimal,
    pub update_at: DateTime<Utc>,
}

impl From<TickerEvent> for MarketData {
    fn from(event: TickerEvent) -> Self {
        Self {
            symbol: event.symbol,
            price: event.current_close,
            price_change_24hr: event.price_change,
            price_change_percent_24hr: event.price_change_percent,
            high_price_24hr: event.high_price,
            low_price_24hr: event.low_price,
            volume_24hr: event.total_base_volume,
            quote_volume_24hr: event.total_quote_volume,
            update_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ohlc {
    pub symbol: String,
    pub interval: String,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub quote_volume: Decimal,
    pub trade_count: u64,
    pub is_closed: bool,
    pub open_time: DateTime<Utc>,
    pub close_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketUpdateEvent {
    TickerUpdated {
        data: MarketData,
        previous_price: Option<Decimal>
    },
    CandleClosed(Ohlc),
}