-- This file should undo anything in `up.sql`
DROP VIEW pay_debit_note_event_read;
DROP VIEW pay_invoice_event_read;

CREATE VIEW pay_debit_note_event_read AS
SELECT
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


CREATE VIEW pay_invoice_event_read AS
SELECT
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

