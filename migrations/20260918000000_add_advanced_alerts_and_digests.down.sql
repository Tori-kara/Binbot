-- Revert market_digest_subscriptions table and condition_payload column
DROP TABLE IF EXISTS market_digest_subscriptions;
ALTER TABLE alerts DROP COLUMN IF EXISTS condition_payload;
