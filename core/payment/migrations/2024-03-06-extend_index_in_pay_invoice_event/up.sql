CREATE TABLE pay_invoice_event_copy(
    invoice_id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    details TEXT NULL,
    PRIMARY KEY(invoice_id, event_type, owner_id),
    FOREIGN KEY(owner_id, invoice_id) REFERENCES pay_invoice (owner_id, id),
    FOREIGN KEY(event_type) REFERENCES pay_event_type (event_type)
);
INSERT INTO pay_invoice_event_copy (invoice_id, owner_id, event_type, timestamp, details)
SELECT invoice_id, owner_id, event_type, timestamp, details FROM pay_invoice_event;

DROP VIEW pay_invoice_event_read;

DROP TABLE IF EXISTS pay_invoice_event;
ALTER TABLE pay_invoice_event_copy RENAME TO pay_invoice_event;

CREATE VIEW pay_invoice_event_read AS
SELECT
    inv.role,
    ie.invoice_id,
    ie.owner_id,
    ie.event_type,
    ie.timestamp,
    ie.details,
    agr.app_session_id
FROM
    pay_invoice_event ie
        INNER JOIN pay_invoice inv ON ie.owner_id = inv.owner_id AND ie.invoice_id = inv.id
        INNER JOIN pay_agreement agr ON ie.owner_id = agr.owner_id AND inv.agreement_id = agr.id;