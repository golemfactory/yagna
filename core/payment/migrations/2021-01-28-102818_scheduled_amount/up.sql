-- HACK: Adding not-null column 'total_amount_scheduled' without default value

PRAGMA foreign_keys=off;

CREATE TABLE pay_agreement_tmp(
    id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    role CHAR(1) NOT NULL CHECK (role in ('R', 'P')),
    peer_id VARCHAR(50) NOT NULL,
    payee_addr VARCHAR(50) NOT NULL,
    payer_addr VARCHAR(50) NOT NULL,
    payment_platform VARCHAR(50) NOT NULL,
    total_amount_due VARCHAR(32) NOT NULL,
    total_amount_accepted VARCHAR(32) NOT NULL,
    total_amount_scheduled VARCHAR(32) NOT NULL,
    total_amount_paid VARCHAR(32) NOT NULL,
    app_session_id VARCHAR(50) NULL,
    PRIMARY KEY (owner_id, id),
    UNIQUE (id, role)
);

CREATE TABLE pay_activity_tmp(
    id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    role CHAR(1) NOT NULL CHECK (role in ('R', 'P')),
    agreement_id VARCHAR(50) NOT NULL,
    total_amount_due VARCHAR(32) NOT NULL,
    total_amount_accepted VARCHAR(32) NOT NULL,
    total_amount_scheduled VARCHAR(32) NOT NULL,
    total_amount_paid VARCHAR(32) NOT NULL,
    PRIMARY KEY(owner_id, id),
    UNIQUE (id, role),
    FOREIGN KEY(owner_id, agreement_id) REFERENCES pay_agreement (owner_id, id)
);

INSERT INTO pay_agreement_tmp(id, owner_id, role, peer_id, payee_addr, payer_addr, payment_platform, total_amount_due, total_amount_accepted, total_amount_scheduled, total_amount_paid, app_session_id)
SELECT id, owner_id, role, peer_id, payee_addr, payer_addr, payment_platform, total_amount_due, total_amount_accepted, total_amount_accepted AS total_amount_scheduled, total_amount_paid, app_session_id FROM pay_agreement;

INSERT INTO pay_activity_tmp(id, owner_id, role, agreement_id, total_amount_due, total_amount_accepted, total_amount_scheduled, total_amount_paid)
SELECT id, owner_id, role, agreement_id, total_amount_due, total_amount_accepted, total_amount_accepted AS total_amount_scheduled, total_amount_paid FROM pay_activity;

DROP VIEW pay_debit_note_event_read;
DROP VIEW pay_invoice_event_read;

DROP TABLE pay_agreement;
DROP TABLE pay_activity;

ALTER TABLE pay_agreement_tmp RENAME TO pay_agreement;
ALTER TABLE pay_activity_tmp RENAME TO pay_activity;


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

