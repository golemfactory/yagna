CREATE TABLE pay_debit_note_event_copy(
    debit_note_id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    details TEXT NULL,
    PRIMARY KEY(debit_note_id, event_type),
    FOREIGN KEY(owner_id, debit_note_id) REFERENCES pay_invoice (owner_id, id),
    FOREIGN KEY(event_type) REFERENCES pay_event_type (event_type)
);
INSERT INTO pay_debit_note_event_copy (debit_note_id, owner_id, event_type, timestamp, details)
SELECT debit_note_id, owner_id, event_type, timestamp, details FROM pay_debit_note_event;

DROP VIEW pay_debit_note_event_read;

DROP TABLE IF EXISTS pay_debit_note_event;
ALTER TABLE pay_debit_note_event_copy RENAME TO pay_debit_note_event;

CREATE VIEW pay_debit_note_event_read AS
SELECT
    inv.role,
    ie.debit_note_id,
    ie.owner_id,
    ie.event_type,
    ie.timestamp,
    ie.details,
    agr.app_session_id
FROM
    pay_debit_note_event ie
        INNER JOIN pay_invoice inv ON ie.owner_id = inv.owner_id AND ie.debit_note_id = inv.id
        INNER JOIN pay_agreement agr ON ie.owner_id = agr.owner_id AND inv.agreement_id = agr.id;
