-- Add up migration script here
CREATE TABLE channels (
    id BIGSERIAL PRIMARY KEY,
    guild_id BIGINT REFERENCES guilds(id) ON DELETE CASCADE,
    channel_id VARCHAR(32) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
)