-- Add skipped column to pay_batch_order_item to properly track items
-- that were skipped due to being below the minimum payment threshold.
ALTER TABLE pay_batch_order_item ADD COLUMN skipped BOOLEAN NOT NULL DEFAULT FALSE;
