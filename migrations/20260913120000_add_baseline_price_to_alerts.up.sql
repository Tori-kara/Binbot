-- Add baseline_price to alerts table for PercentageChange reference
ALTER TABLE alerts ADD COLUMN IF NOT EXISTS baseline_price NUMERIC(20, 8);
