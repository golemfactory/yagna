
DROP VIEW pay_debit_note_event_read;
DROP VIEW pay_invoice_event_read;

-- HACK: All this code below is just to drop column timestamp from table pay_allocation

PRAGMA foreign_keys=off;

CREATE TABLE pay_allocation_tmp(
    id VARCHAR(50) NOT NULL PRIMARY KEY,
    owner_id VARCHAR(50) NOT NULL,
    payment_platform VARCHAR(50) NOT NULL,
    address VARCHAR(50) NOT NULL,
    total_amount VARCHAR(32) NOT NULL,
    spent_amount VARCHAR(32) NOT NULL,
    remaining_amount VARCHAR(32) NOT NULL,
    timeout DATETIME NULL,
    make_deposit BOOLEAN NOT NULL,
    released BOOLEAN NOT NULL DEFAULT FALSE
);

INSERT INTO pay_allocation_tmp(id, owner_id, payment_platform, address, total_amount, spent_amount, remaining_amount, timeout, make_deposit, released)
SELECT id, owner_id, payment_platform, address, total_amount, spent_amount, remaining_amount, timeout, make_deposit, released FROM pay_allocation;


DROP TABLE pay_allocation;

ALTER TABLE pay_allocation_tmp RENAME TO pay_allocation;

-- HACK: All this code below is just to drop column app_session_id from table pay_agreement

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
    total_amount_paid VARCHAR(32) NOT NULL,
    PRIMARY KEY (owner_id, id),
    UNIQUE (id, role)
);

INSERT INTO pay_agreement_tmp(id, owner_id, role, peer_id, payee_addr, payer_addr, payment_platform, total_amount_due, total_amount_accepted, total_amount_paid)
SELECT id, owner_id, role, peer_id, payee_addr, payer_addr, payment_platform, total_amount_due, total_amount_accepted, total_amount_paid FROM pay_agreement;

DROP TABLE pay_agreement;

ALTER TABLE pay_agreement_tmp RENAME TO pay_agreement;

PRAGMA foreign_keys=on;
