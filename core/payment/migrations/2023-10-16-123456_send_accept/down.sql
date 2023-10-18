ALTER TABLE pay_invoice REMOVE COLUMN send_accept;
ALTER TABLE pay_debit_note REMOVE COLUMN send_accept;
ALTER TABLE pay_payment REMOVE COLUMN send_payment;

DROP INDEX pay_invoice_send_accept_idx;
DROP INDEX pay_debit_note_send_accept_idx;
DROP INDEX pay_payment_send_payment_idx;

DROP TABLE pay_sync_needed_notifs;