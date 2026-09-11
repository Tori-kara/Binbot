-- Add up migration script here
CREATE TABLE alerts (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT REFERENCES users(id) ON DELETE CASCADE,
    channel_id BIGINT REFERENCES channels(id) ON DELETE CASCADE,
    symbol VARCHAR(20) NOT NULL,
    condition_type VARCHAR(32) NOT NULL,
    threshold NUMERIC(20, 8) NOT NULL,
    cooldown_seconds INT NOT NULL DEFAULT 1800,
    last_triggered_at TIMESTAMP,
    is_triggered BOOLEAN NOT NULL DEFAULT FALSE,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_alers_lookup ON alerts(symbol, enabled) WHERE enabled = TRUE;
CREATE INDEX idx_alerts_user ON alerts(user_id);