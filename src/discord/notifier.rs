use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use serenity::all::{ChannelId, CreateMessage, Http};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::alerts::AlertNotification;
use crate::discord::embeds::create_alert_triggered_embed;

/// Enforces Discord's channel rate limit: maximum 5 messages per 5 seconds per channel
#[derive(Debug)]
pub struct ChannelRateLimiter {
    channel_history: HashMap<String, VecDeque<Instant>>,
    max_burst: usize,
    window: Duration,
}

impl Default for ChannelRateLimiter {
    fn default() -> Self {
        Self::new(5, Duration::from_secs(5))
    }
}

impl ChannelRateLimiter {
    pub fn new(max_burst: usize, window: Duration) -> Self {
        Self {
            channel_history: HashMap::new(),
            max_burst,
            window,
        }
    }

    /// Blocks asynchronously until a dispatch slot is available for this channel
    pub async fn acquire_slot(&mut self, channel_id: &str) {
        let history = self
            .channel_history
            .entry(channel_id.to_string())
            .or_default();

        loop {
            let now = Instant::now();

            // Purge expired timestamps outside the rolling window
            while let Some(&oldest) = history.front() {
                if now.duration_since(oldest) >= self.window {
                    history.pop_front();
                } else {
                    break;
                }
            }

            if history.len() < self.max_burst {
                // Slot is open
                history.push_back(now);
                return;
            }

            // At capacity: wait until the oldest recorded message falls out of the window
            if let Some(&oldest) = history.front() {
                let elapsed = now.duration_since(oldest);
                if elapsed < self.window {
                    let wait_time = self.window - elapsed + Duration::from_millis(20);
                    tokio::time::sleep(wait_time).await;
                }
            }
        }
    }
}

/// Asynchronous rate-limited alert dispatcher
pub struct AlertDispatcher {
    http: Arc<Http>,
    limiter: ChannelRateLimiter,
}

impl AlertDispatcher {
    pub fn new(http: Arc<Http>) -> Self {
        Self {
            http,
            limiter: ChannelRateLimiter::default(),
        }
    }

    /// Spawns and runs the dispatcher worker consuming incoming notifications
    pub async fn run(mut self, mut rx: mpsc::Receiver<AlertNotification>) {
        info!("✓ Rate-limited Discord Alert Dispatcher started (5 msgs / 5 sec channel limit)");

        while let Some(notif) = rx.recv().await {
            let channel_u64: u64 = match notif.channel_discord_id.parse() {
                Ok(id) => id,
                Err(e) => {
                    error!(
                        channel = %notif.channel_discord_id,
                        "Failed to parse Discord channel ID: {e}"
                    );
                    continue;
                }
            };

            // Acquire rate-limited slot for this channel
            self.limiter.acquire_slot(&notif.channel_discord_id).await;

            let embed = create_alert_triggered_embed(&notif);
            let user_mention = format!("<@{}>", notif.user_discord_id);
            let message = CreateMessage::new().content(user_mention).embed(embed);

            let channel_id = ChannelId::new(channel_u64);
            match channel_id.send_message(&self.http, message).await {
                Ok(_) => {
                    info!(
                        alert_id = notif.alert_id,
                        symbol = %notif.symbol,
                        channel = %notif.channel_discord_id,
                        "✓ Alert notification successfully dispatched to Discord"
                    );
                }
                Err(e) => {
                    warn!(
                        alert_id = notif.alert_id,
                        symbol = %notif.symbol,
                        channel = %notif.channel_discord_id,
                        "Failed to send alert embed to Discord channel: {e}"
                    );
                }
            }
        }

        warn!("Alert Dispatcher stopped: notification channel closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_permits_up_to_burst() {
        let mut limiter = ChannelRateLimiter::new(3, Duration::from_millis(200));
        let ch = "test_channel";

        // First 3 slots should acquire immediately
        let start = Instant::now();
        limiter.acquire_slot(ch).await;
        limiter.acquire_slot(ch).await;
        limiter.acquire_slot(ch).await;
        assert!(start.elapsed() < Duration::from_millis(50));

        // 4th slot must wait for the window to clear
        limiter.acquire_slot(ch).await;
        assert!(start.elapsed() >= Duration::from_millis(150));
    }
}
