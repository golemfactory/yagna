ALTER TABLE pay_invoice ADD COLUMN send_accept BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE pay_debit_note ADD COLUMN send_accept BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE pay_payment ADD COLUMN send_payment BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX pay_invoice_send_accept_idx ON pay_invoice (send_accept);
CREATE INDEX pay_debit_note_send_accept_idx ON pay_debit_note (send_accept);
CREATE INDEX pay_payment_send_payment_idx ON pay_payment (send_payment);

CREATE TABLE pay_sync_needed_notifs(
    id VARCHAR(50) NOT NULL PRIMARY KEY,
    last_ping DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    retries INT NOT NULL DEFAULT(0)
);