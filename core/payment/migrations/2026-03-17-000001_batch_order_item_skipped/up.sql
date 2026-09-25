-- Add skipped column to pay_batch_order_item to properly track items
-- that were skipped due to being below the minimum payment threshold.
ALTER TABLE pay_batch_order_item ADD COLUMN skipped BOOLEAN NOT NULL DEFAULT FALSE;

-- Convert the synthetic payment IDs used by older versions into the explicit
-- skipped state. Paid rows are left untouched to preserve their payment data.
UPDATE pay_batch_order_item
SET skipped = TRUE, payment_id = NULL
WHERE payment_id GLOB 'SKIPPED_PAYMENT_*'
  AND paid = FALSE;
