use chrono::DateTime;
use tokio::sync::broadcast;
use tracing::{debug, trace};

use crate::binance::BinanceEvent;
use crate::market::models::{MarketData, MarketUpdateEvent, Ohlc};
use crate::market::state::MarketState;

/// Processes raw Binance exchange events into normalized domain models,
/// updates the in-memory `MarketState`, and broadcasts `MarketUpdateEvent`s
/// for downstream consumers (such as the Alert Engine).
#[derive(Debug, Clone)]
pub struct MarketProcessor {
    state: MarketState,
    update_tx: broadcast::Sender<MarketUpdateEvent>,
}

impl MarketProcessor {
    pub fn new(state: MarketState, update_tx: broadcast::Sender<MarketUpdateEvent>) -> Self {
        Self { state, update_tx }
    }

    /// Access the underlying `MarketState`
    pub fn state(&self) -> &MarketState {
        &self.state
    }

    /// Access the broadcast sender for market update events
    pub fn update_sender(&self) -> &broadcast::Sender<MarketUpdateEvent> {
        &self.update_tx
    }

    /// Normalizes and processes a single incoming `BinanceEvent`
    pub async fn process_event(&self, event: BinanceEvent) {
        match event {
            BinanceEvent::Ticker(ticker) => {
                let symbol = ticker.symbol.clone();
                let market_data: MarketData = ticker.into();

                // Update in-memory state and retrieve previous price
                let previous_price = self.state.update(market_data.clone()).await;

                trace!(
                    symbol = %symbol,
                    price = %market_data.price,
                    prev_price = ?previous_price,
                    "Normalized ticker update processed"
                );

                // Broadcast event for alerts and downstream listeners
                let _ = self.update_tx.send(MarketUpdateEvent::TickerUpdated {
                    data: market_data,
                    previous_price,
                });
            }
            BinanceEvent::Kline(kline_event) => {
                let k = kline_event.kline;
                // Only broadcast completed candles for signals/alerts
                if k.is_closed {
                    let open_time = DateTime::from_timestamp_millis(k.start_time as i64)
                        .unwrap_or_else(chrono::Utc::now);
                    let close_time = DateTime::from_timestamp_millis(k.end_time as i64)
                        .unwrap_or_else(chrono::Utc::now);

                    let ohlc = Ohlc {
                        symbol: k.symbol.clone(),
                        interval: k.interval,
                        open: k.open_price,
                        high: k.high_price,
                        low: k.low_price,
                        close: k.close_price,
                        volume: k.base_volume,
                        quote_volume: k.quote_volume,
                        trade_count: k.trade_count,
                        is_closed: true,
                        open_time,
                        close_time,
                    };

                    debug!(
                        symbol = %k.symbol,
                        interval = %ohlc.interval,
                        close = %ohlc.close,
                        "Closed candlestick processed"
                    );

                    let _ = self.update_tx.send(MarketUpdateEvent::CandleClosed(ohlc));
                }
            }
            BinanceEvent::MiniTicker(_)
            | BinanceEvent::Trade(_)
            | BinanceEvent::AggTrade(_) => {
                // Secondary streams can be routed here as needed
            }
        }
    }

    /// Starts an asynchronous worker loop consuming events from a Binance event receiver
    pub async fn run(self, mut event_rx: broadcast::Receiver<BinanceEvent>) {
        debug!("MarketProcessor worker started");
        while let Ok(event) = event_rx.recv().await {
            self.process_event(event).await;
        }
        debug!("MarketProcessor worker terminated");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binance::TickerEvent;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_process_ticker_event() {
        let state = MarketState::new();
        let (tx, mut rx) = broadcast::channel(16);
        let processor = MarketProcessor::new(state.clone(), tx);

        let ticker = TickerEvent {
            event_time: 123456789,
            symbol: "BTCUSDT".to_string(),
            price_change: dec!(150.00),
            price_change_percent: dec!(0.25),
            weighted_avg_price: dec!(60000.00),
            prev_close_price: dec!(59850.00),
            current_close: dec!(60000.00),
            last_quantity: dec!(0.5),
            best_bid_price: dec!(59999.00),
            best_bid_quantity: dec!(1.2),
            best_ask_price: dec!(60001.00),
            best_ask_quantity: dec!(2.5),
            open_price: dec!(59850.00),
            high_price: dec!(60500.00),
            low_price: dec!(59500.00),
            total_base_volume: dec!(1000.50),
            total_quote_volume: dec!(60030000.00),
            stat_open_time: 123400000,
            stat_close_time: 123486400,
            first_trade_id: 1000,
            last_trade_id: 2000,
            total_trades: 1001,
        };

        processor.process_event(BinanceEvent::Ticker(ticker.clone())).await;

        // Verify MarketState has the updated price
        let price = state.get_price("BTCUSDT").await;
        assert_eq!(price, Some(dec!(60000.00)));

        // Verify broadcast event was received
        if let Ok(MarketUpdateEvent::TickerUpdated { data, previous_price }) = rx.recv().await {
            assert_eq!(data.symbol, "BTCUSDT");
            assert_eq!(data.price, dec!(60000.00));
            assert_eq!(previous_price, None);
        } else {
            panic!("Expected TickerUpdated event");
        }

        // Process a second event to verify previous_price tracking
        let mut second_ticker = ticker;
        second_ticker.current_close = dec!(60500.00);
        processor.process_event(BinanceEvent::Ticker(second_ticker)).await;

        if let Ok(MarketUpdateEvent::TickerUpdated { data, previous_price }) = rx.recv().await {
            assert_eq!(data.price, dec!(60500.00));
            assert_eq!(previous_price, Some(dec!(60000.00)));
        } else {
            panic!("Expected second TickerUpdated event");
        }
    }
}
