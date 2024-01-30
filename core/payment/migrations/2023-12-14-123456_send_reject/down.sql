DROP INDEX pay_invoice_send_reject_idx;

ALTER TABLE pay_invoice REMOVE COLUMN send_reject;
