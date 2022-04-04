-- Your SQL goes here
DROP VIEW pay_debit_note_event_read;

CREATE VIEW pay_debit_note_event_read AS
SELECT
    dn.role_id,
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
    INNER JOIN pay_agreement agr ON dne.owner_id = agr.owner_id AND act.agreement_id = agr.id;


