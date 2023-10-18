ALTER TABLE pay_invoice REMOVE COLUMN send_accept;
ALTER TABLE pay_debit_note REMOVE COLUMN send_accept;
ALTER TABLE pay_payment REMOVE COLUMN send_payment;

DROP TABLE pay_sync_needed_notifs;