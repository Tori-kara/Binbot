-- Add condition_payload JSONB column to alerts table for multi-condition and rolling-window alerts
ALTER TABLE alerts ADD COLUMN IF NOT EXISTS condition_payload JSONB;

-- Create market_digest_subscriptions table for scheduled daily digests
CREATE TABLE IF NOT EXISTS market_digest_subscriptions (
    id BIGSERIAL PRIMARY KEY,
    guild_id BIGINT REFERENCES guilds(id) ON DELETE CASCADE,
    channel_id BIGINT REFERENCES channels(id) ON DELETE CASCADE UNIQUE,
    scheduled_time_utc VARCHAR(10) NOT NULL DEFAULT '08:00',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_market_digest_channel ON market_digest_subscriptions(channel_id);
CREATE INDEX IF NOT EXISTS idx_market_digest_enabled ON market_digest_subscriptions(enabled) WHERE enabled = TRUE;
