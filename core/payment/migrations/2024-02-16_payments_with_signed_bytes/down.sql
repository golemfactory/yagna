ALTER TABLE pay_payment DROP COLUMN signature;
ALTER TABLE pay_payment DROP COLUMN signed_bytes;

CREATE TABLE pay_invoice_event_copy(
    invoice_id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    details TEXT NULL,
    PRIMARY KEY(invoice_id, event_type),
    FOREIGN KEY(owner_id, invoice_id) REFERENCES pay_invoice (owner_id, id),
    FOREIGN KEY(event_type) REFERENCES pay_event_type (event_type)
);
INSERT INTO pay_invoice_event_copy (invoice_id, owner_id, event_type, timestamp, details)
SELECT invoice_id, owner_id, event_type, timestamp, details FROM pay_invoice_event;
PRAGMA foreign_keys = OFF;
PRAGMA defer_foreign_keys = ON;
DROP TABLE IF EXISTS pay_invoice_event;
ALTER TABLE pay_invoice_event_copy RENAME TO pay_invoice_event;
PRAGMA foreign_keys = ON;