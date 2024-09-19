CREATE TABLE pay_debit_note_event_copy(
    debit_note_id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    details TEXT NULL,
    PRIMARY KEY(debit_note_id, event_type, owner_id),
    FOREIGN KEY(owner_id, debit_note_id) REFERENCES pay_debit_note (owner_id, id),
    FOREIGN KEY(event_type) REFERENCES pay_event_type (event_type)
);
INSERT INTO pay_debit_note_event_copy (debit_note_id, owner_id, event_type, timestamp, details)
SELECT debit_note_id, owner_id, event_type, timestamp, details FROM pay_debit_note_event;

DROP VIEW pay_debit_note_event_read;

DROP TABLE IF EXISTS pay_debit_note_event;
ALTER TABLE pay_debit_note_event_copy RENAME TO pay_debit_note_event;

CREATE VIEW pay_debit_note_event_read AS
SELECT
    dn.role,
    dne.debit_note_id,
    dne.owner_id,
    dne.event_type,
    dne.timestamp,
    dne.details,
    agr.app_session_id
FROM
    pay_debit_note_event dne
    INNER JOIN pay_debit_note dn ON dne.owner_id = dn.owner_id AND dne.debit_note_id = dn.id
    INNER JOIN pay_activity act ON dne.owner_id = act.owner_id AND dn.activity_id = act.id
    INNER JOIN pay_agreement agr ON dne.owner_id = agr.owner_id AND act.agreement_id = agr.id
