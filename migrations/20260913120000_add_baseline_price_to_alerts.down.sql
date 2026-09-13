-- Drop baseline_price column from alerts table
ALTER TABLE alerts DROP COLUMN IF EXISTS baseline_price;
