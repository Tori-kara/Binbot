use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::binance::client::BinanceWsConfig;
use crate::binance::models::{
    BinanceEvent, CombinedStreamPayload, SubscriptionRequest, SubscriptionResponse,
};
use crate::error::AppError;

#[derive(Debug)]
enum StreamCommand {
    Subscribe(Vec<String>),
    Unsubscribe(Vec<String>),
    Shutdown,
}

/// A resilient, auto-reconnecting Binance WebSocket client.
/// Manages subscriptions and broadcasts incoming events to subscribers.
#[derive(Clone)]
pub struct BinanceWebSocketClient {
    cmd_tx: mpsc::Sender<StreamCommand>,
    event_tx: broadcast::Sender<BinanceEvent>,
}

impl BinanceWebSocketClient {
    /// Spawns the WebSocket background worker and returns a client handle.
    pub fn connect(config: BinanceWsConfig) -> Result<Self, AppError> {
        let (cmd_tx, cmd_rx) = mpsc::channel(128);
        let (event_tx, _) = broadcast::channel(config.event_buffer_size);

        let worker = WebSocketWorker {
            config,
            cmd_rx,
            event_tx: event_tx.clone(),
            active_subscriptions: HashSet::new(),
            request_id: Arc::new(AtomicU64::new(1)),
        };

        tokio::spawn(async move {
            worker.run().await;
        });

        Ok(Self { cmd_tx, event_tx })
    }

    /// Subscribes to the broadcast channel of incoming Binance events
    pub fn subscribe_events(&self) -> broadcast::Receiver<BinanceEvent> {
        self.event_tx.subscribe()
    }

    /// Dynamically subscribes to one or more Binance streams (e.g. "btcusdt@ticker", "ethusdt@kline_1m")
    pub async fn subscribe<I, S>(&self, streams: I) -> Result<(), AppError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let stream_names: Vec<String> = streams
            .into_iter()
            .map(|s| s.as_ref().to_ascii_lowercase())
            .collect();

        if stream_names.is_empty() {
            return Ok(());
        }

        self.cmd_tx
            .send(StreamCommand::Subscribe(stream_names))
            .await
            .map_err(|_| AppError::ChannelSend("WebSocket worker has shut down".to_string()))
    }

    /// Dynamically unsubscribes from one or more Binance streams
    pub async fn unsubscribe<I, S>(&self, streams: I) -> Result<(), AppError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let stream_names: Vec<String> = streams
            .into_iter()
            .map(|s| s.as_ref().to_ascii_lowercase())
            .collect();

        if stream_names.is_empty() {
            return Ok(());
        }

        self.cmd_tx
            .send(StreamCommand::Unsubscribe(stream_names))
            .await
            .map_err(|_| AppError::ChannelSend("WebSocket worker has shut down".to_string()))
    }

    /// Cleanly requests the WebSocket worker to shut down
    pub async fn shutdown(&self) -> Result<(), AppError> {
        self.cmd_tx
            .send(StreamCommand::Shutdown)
            .await
            .map_err(|_| AppError::ChannelSend("WebSocket worker already stopped".to_string()))
    }
}

struct WebSocketWorker {
    config: BinanceWsConfig,
    cmd_rx: mpsc::Receiver<StreamCommand>,
    event_tx: broadcast::Sender<BinanceEvent>,
    active_subscriptions: HashSet<String>,
    request_id: Arc<AtomicU64>,
}

impl WebSocketWorker {
    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn run(mut self) {
        let mut reconnect_delay = self.config.initial_reconnect_delay;

        loop {
            tracing::debug!(
                endpoint = %self.config.base_url,
                active_streams = self.active_subscriptions.len(),
                "Connecting to Binance WebSocket..."
            );

            let ws_url = match Url::parse(&self.config.base_url) {
                Ok(url) => url,
                Err(e) => {
                    tracing::error!(error = %e, "Invalid Binance WebSocket URL");
                    return;
                }
            };

            match connect_async(ws_url.as_str()).await {
                Ok((ws_stream, response)) => {
                    tracing::debug!(
                        status = %response.status(),
                        "Successfully connected to Binance WebSocket"
                    );

                    // Reset backoff upon successful handshake
                    reconnect_delay = self.config.initial_reconnect_delay;

                    let (mut ws_writer, mut ws_reader) = ws_stream.split();

                    // Automatically re-subscribe to all active streams on reconnect
                    if !self.active_subscriptions.is_empty() {
                        let streams: Vec<String> = self.active_subscriptions.iter().cloned().collect();
                        let sub_req = SubscriptionRequest::subscribe(streams, self.next_id());
                        if let Ok(payload) = serde_json::to_string(&sub_req) {
                            tracing::debug!(
                                count = self.active_subscriptions.len(),
                                "Re-subscribing active streams to Binance"
                            );
                            if let Err(e) = ws_writer.send(Message::Text(payload.into())).await {
                                tracing::warn!(error = %e, "Failed to send initial subscriptions");
                            }
                        }
                    }

                    // Event loop for active connection
                    let mut should_reconnect = true;
                    'connection: loop {
                        tokio::select! {
                            // Outgoing command received from client handle
                            cmd = self.cmd_rx.recv() => {
                                match cmd {
                                    Some(StreamCommand::Subscribe(streams)) => {
                                        let mut new_streams = Vec::new();
                                        for s in streams {
                                            if self.active_subscriptions.insert(s.clone()) {
                                                new_streams.push(s);
                                            }
                                        }

                                        if !new_streams.is_empty() {
                                            let sub_req = SubscriptionRequest::subscribe(new_streams, self.next_id());
                                            if let Ok(payload) = serde_json::to_string(&sub_req) {
                                                tracing::debug!(payload = %payload, "Sending SUBSCRIBE payload to Binance");
                                                if let Err(e) = ws_writer.send(Message::Text(payload.into())).await {
                                                    tracing::warn!(error = %e, "Failed to send SUBSCRIBE message");
                                                    break 'connection;
                                                }
                                            }
                                        }
                                    }
                                    Some(StreamCommand::Unsubscribe(streams)) => {
                                        let mut remove_streams = Vec::new();
                                        for s in streams {
                                            if self.active_subscriptions.remove(&s) {
                                                remove_streams.push(s);
                                            }
                                        }

                                        if !remove_streams.is_empty() {
                                            let unsub_req = SubscriptionRequest::unsubscribe(remove_streams, self.next_id());
                                            if let Ok(payload) = serde_json::to_string(&unsub_req) {
                                                if let Err(e) = ws_writer.send(Message::Text(payload.into())).await {
                                                    tracing::warn!(error = %e, "Failed to send UNSUBSCRIBE message");
                                                    break 'connection;
                                                }
                                            }
                                        }
                                    }
                                    Some(StreamCommand::Shutdown) => {
                                        tracing::info!("Shutting down Binance WebSocket worker");
                                        let _ = ws_writer.send(Message::Close(None)).await;
                                        should_reconnect = false;
                                        break 'connection;
                                    }
                                    None => {
                                        tracing::debug!("Client handle dropped; shutting down worker");
                                        should_reconnect = false;
                                        break 'connection;
                                    }
                                }
                            }

                            // Incoming frame received from WebSocket
                            msg = ws_reader.next() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        self.handle_incoming_text(text.as_str());
                                    }
                                    Some(Ok(Message::Ping(data))) => {
                                        tracing::trace!("Received ping frame, sending pong");
                                        if let Err(e) = ws_writer.send(Message::Pong(data)).await {
                                            tracing::warn!(error = %e, "Failed to respond with pong frame");
                                            break 'connection;
                                        }
                                    }
                                    Some(Ok(Message::Pong(_))) => {
                                        tracing::trace!("Received pong frame");
                                    }
                                    Some(Ok(Message::Close(frame))) => {
                                        tracing::warn!(frame = ?frame, "Binance WebSocket connection closed by server");
                                        break 'connection;
                                    }
                                    Some(Ok(Message::Binary(_))) => {
                                        // Binance market streams use text JSON frames
                                    }
                                    Some(Ok(Message::Frame(_))) => {}
                                    Some(Err(e)) => {
                                        tracing::warn!(error = %e, "WebSocket read error");
                                        break 'connection;
                                    }
                                    None => {
                                        tracing::warn!("WebSocket stream ended unexpectedly");
                                        break 'connection;
                                    }
                                }
                            }
                        }
                    }

                    if !should_reconnect {
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to connect to Binance WebSocket");
                }
            }

            // Exponential backoff before reconnecting
            tracing::debug!(delay_secs = reconnect_delay.as_secs(), "Waiting before reconnecting...");
            tokio::time::sleep(reconnect_delay).await;
            reconnect_delay = (reconnect_delay * 2).min(self.config.max_reconnect_delay);
        }
    }

    fn handle_incoming_text(&self, text: &str) {
        // 1. Try combined stream payload: {"stream": "...", "data": {...} or [...]}
        if let Ok(combined) = serde_json::from_str::<CombinedStreamPayload<serde_json::Value>>(text) {
            if let Ok(event) = serde_json::from_value::<BinanceEvent>(combined.data.clone()) {
                let _ = self.event_tx.send(event);
                return;
            }
            if let Ok(events) = serde_json::from_value::<Vec<BinanceEvent>>(combined.data) {
                for event in events {
                    let _ = self.event_tx.send(event);
                }
                return;
            }
        }

        // 2. Try direct single event payload: {"e": "...", ...}
        if let Ok(event) = serde_json::from_str::<BinanceEvent>(text) {
            let _ = self.event_tx.send(event);
            return;
        }

        // 3. Try direct array of events: [{"e": "...", ...}, ...] (e.g. !ticker@arr)
        if let Ok(raw_array) = serde_json::from_str::<Vec<serde_json::Value>>(text) {
            let mut count = 0;
            for val in raw_array {
                if let Ok(event) = serde_json::from_value::<BinanceEvent>(val) {
                    let _ = self.event_tx.send(event);
                    count += 1;
                }
            }
            if count > 0 {
                return;
            }
        }

        // 4. Try subscription response: {"result": null, "id": 1}
        if let Ok(sub_resp) = serde_json::from_str::<SubscriptionResponse>(text) {
            if let Some(err) = sub_resp.error {
                tracing::error!(id = sub_resp.id, code = err.code, msg = %err.msg, "Binance subscription error");
            } else {
                tracing::info!(id = sub_resp.id, result = ?sub_resp.result, "Binance subscription confirmed");
            }
            return;
        }

        tracing::trace!(payload = text, "Ignored unhandled WebSocket payload");
    }
}
