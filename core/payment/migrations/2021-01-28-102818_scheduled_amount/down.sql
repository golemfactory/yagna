-- HACK: removing column 'total_amount_scheduled'

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
    total_amount_paid VARCHAR(32) NOT NULL,
    PRIMARY KEY(owner_id, id),
    UNIQUE (id, role),
    FOREIGN KEY(owner_id, agreement_id) REFERENCES pay_agreement (owner_id, id)
);

INSERT INTO pay_agreement_tmp(id, owner_id, role, peer_id, payee_addr, payer_addr, payment_platform, total_amount_due, total_amount_accepted, total_amount_paid, app_session_id)
SELECT id, owner_id, role, peer_id, payee_addr, payer_addr, payment_platform, total_amount_due, total_amount_accepted, total_amount_paid, app_session_id FROM pay_agreement;

INSERT INTO pay_activity_tmp(id, owner_id, role, agreement_id, total_amount_due, total_amount_accepted, total_amount_paid)
SELECT id, owner_id, role, agreement_id, total_amount_due, total_amount_accepted, total_amount_paid FROM pay_activity;

DROP TABLE pay_agreement;
DROP TABLE pay_activity;

ALTER TABLE pay_agreement_tmp RENAME TO pay_agreement;
ALTER TABLE pay_activity_tmp RENAME TO pay_activity;

PRAGMA foreign_keys=on;
