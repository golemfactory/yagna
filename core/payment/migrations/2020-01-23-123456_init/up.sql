CREATE TABLE pay_agreement(
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

CREATE TABLE pay_activity(
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

CREATE TABLE pay_document_status(
    status VARCHAR(50) NOT NULL PRIMARY KEY
);

INSERT INTO pay_document_status(status) VALUES('ISSUED');
INSERT INTO pay_document_status(status) VALUES('RECEIVED');
INSERT INTO pay_document_status(status) VALUES('ACCEPTED');
INSERT INTO pay_document_status(status) VALUES('REJECTED');
INSERT INTO pay_document_status(status) VALUES('FAILED');
INSERT INTO pay_document_status(status) VALUES('SETTLED');
INSERT INTO pay_document_status(status) VALUES('CANCELLED');


CREATE TABLE pay_debit_note(
    id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    role CHAR(1) NOT NULL CHECK (role in ('R', 'P')),
    previous_debit_note_id VARCHAR(50) NULL,
    activity_id VARCHAR(50) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'ISSUED',
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    total_amount_due VARCHAR(32) NOT NULL,
    usage_counter_vector BLOB NULL,
    payment_due_date DATETIME NULL,
    PRIMARY KEY(owner_id, id),
    UNIQUE (id, role),
    FOREIGN KEY(owner_id, previous_debit_note_id) REFERENCES pay_debit_note (owner_id, id),
    FOREIGN KEY(owner_id, activity_id) REFERENCES pay_activity (owner_id, id),
    FOREIGN KEY(status) REFERENCES pay_document_status (status)
);

CREATE TABLE pay_invoice(
    id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    role CHAR(1) NOT NULL CHECK (role in ('R', 'P')),
    agreement_id VARCHAR(50) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'ISSUED',
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    amount VARCHAR(32) NOT NULL,
    payment_due_date DATETIME NOT NULL,
    PRIMARY KEY(owner_id, id),
    UNIQUE (id, role),
    FOREIGN KEY(owner_id, agreement_id) REFERENCES pay_agreement (owner_id, id),
    FOREIGN KEY(status) REFERENCES pay_document_status (status)
);

CREATE TABLE pay_invoice_x_activity(
    invoice_id VARCHAR(50) NOT NULL,
    activity_id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    PRIMARY KEY(owner_id, invoice_id, activity_id),
    FOREIGN KEY(owner_id, invoice_id) REFERENCES pay_invoice (owner_id, id),
    FOREIGN KEY(owner_id, activity_id) REFERENCES pay_activity (owner_id, id)
);

CREATE TABLE pay_allocation(
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

CREATE TABLE pay_payment(
    id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    peer_id VARCHAR(50) NOT NULL,
    payee_addr VARCHAR(50) NOT NULL,
    payer_addr VARCHAR(50) NOT NULL,
    payment_platform VARCHAR(50) NOT NULL,
    role CHAR(1) NOT NULL CHECK (role in ('R', 'P')),
    amount VARCHAR(32) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    details BLOB NOT NULL,
    PRIMARY KEY(owner_id, id),
    UNIQUE (id, role)
);

CREATE TABLE pay_activity_payment(
    payment_id VARCHAR(50) NOT NULL,
    activity_id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    amount VARCHAR(32) NOT NULL,
    allocation_id VARCHAR(50) NULL,
    PRIMARY KEY(owner_id, payment_id, activity_id),
    FOREIGN KEY(owner_id, payment_id) REFERENCES pay_payment(owner_id, id),
    FOREIGN KEY(owner_id, activity_id) REFERENCES pay_activity(owner_id, id),
    FOREIGN KEY(allocation_id) REFERENCES pay_allocation(id) ON DELETE SET NULL
);

CREATE TABLE pay_agreement_payment(
    payment_id VARCHAR(50) NOT NULL,
    agreement_id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    amount VARCHAR(32) NOT NULL,
    allocation_id VARCHAR(50) NULL,
    PRIMARY KEY(owner_id, payment_id, agreement_id),
    FOREIGN KEY(owner_id, payment_id) REFERENCES pay_payment(owner_id, id),
    FOREIGN KEY(owner_id, agreement_id) REFERENCES pay_agreement(owner_id, id),
    FOREIGN KEY(allocation_id) REFERENCES pay_allocation(id) ON DELETE SET NULL
);

CREATE TABLE pay_event_type(
    event_type VARCHAR(50) NOT NULL PRIMARY KEY,
    role CHAR(1) NOT NULL CHECK (role in ('R', 'P'))
);

INSERT INTO pay_event_type(event_type, role) VALUES('RECEIVED', 'R');
INSERT INTO pay_event_type(event_type, role) VALUES('ACCEPTED', 'P');
INSERT INTO pay_event_type(event_type, role) VALUES('REJECTED', 'P');
INSERT INTO pay_event_type(event_type, role) VALUES('CANCELLED', 'R');
INSERT INTO pay_event_type(event_type, role) VALUES('SETTLED', 'P');

CREATE TABLE pay_debit_note_event(
    debit_note_id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    details TEXT NULL,
    PRIMARY KEY(debit_note_id, event_type),
    FOREIGN KEY(owner_id, debit_note_id) REFERENCES pay_debit_note (owner_id, id),
    FOREIGN KEY(event_type) REFERENCES pay_event_type (event_type)
);

CREATE TABLE pay_invoice_event(
    invoice_id VARCHAR(50) NOT NULL,
    owner_id VARCHAR(50) NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    details TEXT NULL,
    PRIMARY KEY(invoice_id, event_type),
    FOREIGN KEY(owner_id, invoice_id) REFERENCES pay_invoice (owner_id, id),
    FOREIGN KEY(event_type) REFERENCES pay_event_type (event_type)
);

CREATE TABLE pay_order(
    id VARCHAR(50) NOT NULL,
    driver VARCHAR(50) NOT NULL,
    amount VARCHAR(32) NOT NULL,
    payee_id VARCHAR(50) NOT NULL,
    payer_id VARCHAR(50) NOT NULL,
    payee_addr VARCHAR(50) NOT NULL,
    payer_addr VARCHAR(50) NOT NULL,
    payment_platform VARCHAR(50) NOT NULL,
    invoice_id VARCHAR(50) NULL UNIQUE,
    debit_note_id VARCHAR(50) NULL UNIQUE,
    allocation_id VARCHAR(50) NOT NULL,
    is_paid BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY(id, driver),
    FOREIGN KEY(payer_id, invoice_id) REFERENCES pay_invoice (owner_id, id),
    FOREIGN KEY(payer_id, debit_note_id) REFERENCES pay_debit_note (owner_id, id),
    FOREIGN KEY(allocation_id) REFERENCES pay_allocation (id),
    CHECK ((invoice_id IS NULL) <> (debit_note_id IS NULL))
);
