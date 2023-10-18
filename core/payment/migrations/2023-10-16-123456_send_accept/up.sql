ALTER TABLE pay_invoice ADD COLUMN send_accept BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE pay_debit_note ADD COLUMN send_accept BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE pay_payment ADD COLUMN send_payment BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE pay_sync_needed_notifs(
    id VARCHAR(50) NOT NULL PRIMARY KEY,
    last_ping DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    retries INT NOT NULL DEFAULT(0)
);