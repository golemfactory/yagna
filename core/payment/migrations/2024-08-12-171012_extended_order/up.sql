-- Your SQL goes here
CREATE TABLE pay_batch_order(
    id              VARCHAR (50) NOT NULL,
    ts              DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    owner_id        VARCHAR(50) NOT NULL,
    payer_addr      VARCHAR(50) NOT NULL,
    platform        VARCHAR(50) NOT NULL,
    total_amount    VARCHAR(32) NOT NULL,
    paid_amount     VARCHAR(32) NOT NULL,
    paid            BOOLEAN NOT NULL DEFAULT FALSE,

    CONSTRAINT pay_batch_order_pk PRIMARY KEY(owner_id, id)
);

CREATE INDEX pay_batch_order_ts ON pay_batch_order (ts);

CREATE TABLE pay_batch_order_item(
    order_id        VARCHAR(50) NOT NULL,
    owner_id        VARCHAR(50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    amount          VARCHAR(32) NOT NULL,
    payment_id      VARCHAR(50),
    paid            BOOLEAN NOT NULL DEFAULT FALSE,

    CONSTRAINT pay_batch_order_item_pk PRIMARY KEY (owner_id, order_id, payee_addr),
    CONSTRAINT pay_batch_order_item_fk1 FOREIGN KEY (owner_id, order_id) REFERENCES pay_batch_order(owner_id, id)
);

CREATE TABLE pay_batch_order_item_document(
    order_id        VARCHAR(50) NOT NULL,
    owner_id        VARCHAR(50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    agreement_id    VARCHAR(50) NOT NULL,
    invoice_id      VARCHAR(50) NULL,
    activity_id     VARCHAR(50) NULL,
    debit_note_id   VARCHAR(50) NULL,
    amount          VARCHAR(32) NOT NULL,

    CONSTRAINT pay_batch_order_item_agreement_pk PRIMARY KEY (owner_id, order_id, payee_addr, agreement_id, activity_id),
    CONSTRAINT pay_batch_order_item_agreement_fk1 FOREIGN KEY (owner_id, order_id, payee_addr) REFERENCES pay_batch_order_item(owner_id, order_id, payee_addr),
    CONSTRAINT pay_batch_order_item_agreement_fk2 FOREIGN KEY (owner_id, agreement_id) REFERENCES pay_agreement(owner_id, id),
    CONSTRAINT pay_batch_order_item_agreement_fk3 FOREIGN KEY (owner_id, activity_id) REFERENCES pay_activity(owner_id, id),
    CONSTRAINT pay_batch_order_item_agreement_fk4 FOREIGN KEY (owner_id, invoice_id)
            REFERENCES pay_invoice(owner_id, id)
            ON DELETE SET NULL,
    CONSTRAINT pay_batch_order_item_agreement_fk5 FOREIGN KEY (owner_id, debit_note_id)
            REFERENCES pay_debit_note(owner_id, id)
            ON DELETE SET NULL,
    CHECK ((invoice_id IS NULL) <> (debit_note_id IS NULL))
);

CREATE TABLE pay_batch_cycle
(
    owner_id VARCHAR(50) NOT NULL,
    platform VARCHAR(50) NOT NULL,
    created_ts DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    updated_ts DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    cycle_interval VARCHAR(50),
    cycle_cron VARCHAR(50),
    cycle_last_process DATETIME,
    cycle_next_process DATETIME NOT NULL,
    cycle_max_interval VARCHAR(50) NOT NULL,
    cycle_extra_pay_time VARCHAR(50) NOT NULL,

    CONSTRAINT pay_batch_cycle_pk PRIMARY KEY(owner_id, platform),
    CONSTRAINT pay_batch_cycle_check_1 CHECK((cycle_interval IS NULL) <> (cycle_cron IS NULL))
);
DROP INDEX pay_activity_payment_payment_owner_idx;
DROP INDEX pay_agreement_payment_payment_owner_idx;
DROP TABLE pay_activity_payment;
DROP TABLE pay_agreement_payment;
DROP TABLE pay_order;

DROP INDEX pay_allocation_timestamp_idx;
DROP INDEX pay_allocation_payment_platform_address_idx;
DROP TABLE pay_allocation;

CREATE TABLE pay_allocation
(
    id               VARCHAR(50) NOT NULL,
    owner_id         VARCHAR(50) NOT NULL,
    payment_platform VARCHAR(50) NOT NULL,
    address          VARCHAR(50) NOT NULL,
    avail_amount     VARCHAR(32) NOT NULL,
    spent_amount     VARCHAR(32) NOT NULL,
    created_ts       DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    updated_ts       DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    timeout          DATETIME NOT NULL,
    released         BOOLEAN NOT NULL,
    deposit          TEXT,

    CONSTRAINT pay_allocation_pk PRIMARY KEY(owner_id, id)
);

CREATE INDEX pay_allocation_ppa_idx ON pay_allocation (payment_platform, address);
CREATE INDEX pay_allocation_created_ts ON pay_allocation (created_ts);
CREATE INDEX pay_allocation_updated_ts ON pay_allocation (updated_ts);

CREATE TABLE pay_allocation_document
(
    owner_id      VARCHAR(50) NOT NULL,
    allocation_id VARCHAR(50) NOT NULL,
    agreement_id  VARCHAR(50) NOT NULL,
    invoice_id    VARCHAR(50),
    activity_id   VARCHAR(50),
    debit_note_id VARCHAR(50),
    spent_amount  VARCHAR(32) NOT NULL,

    CONSTRAINT pay_allocation_document_pk PRIMARY KEY (owner_id, allocation_id, agreement_id, activity_id),
    CONSTRAINT pay_allocation_document_fk1 FOREIGN KEY (owner_id, allocation_id) REFERENCES pay_allocation(owner_id, id),
    CONSTRAINT pay_allocation_document_fk2 FOREIGN KEY (owner_id, activity_id) REFERENCES pay_activity(owner_id, id),
    CONSTRAINT pay_allocation_document_fk3 FOREIGN KEY (owner_id, agreement_id) REFERENCES pay_agreement(owner_id, id),
    CONSTRAINT pay_allocation_document_fk4 FOREIGN KEY (owner_id, invoice_id)
        REFERENCES pay_invoice(owner_id, id)
        ON DELETE SET NULL,
    CONSTRAINT pay_allocation_document_fk5 FOREIGN KEY (owner_id, debit_note_id)
        REFERENCES pay_debit_note(owner_id, id)
        ON DELETE SET NULL,
    CHECK ((invoice_id IS NULL) <> (debit_note_id IS NULL))
);

CREATE TABLE pay_payment_document(
    owner_id      VARCHAR(50) NOT NULL,
    payment_id    VARCHAR(50) NOT NULL,
    agreement_id  VARCHAR(50) NOT NULL,
    invoice_id    VARCHAR(50),
    activity_id   VARCHAR(50),
    debit_note_id VARCHAR(50),
    amount        VARCHAR(32) NOT NULL,

    CONSTRAINT pay_payment_document_pk PRIMARY KEY (owner_id, payment_id, agreement_id, activity_id),
    CONSTRAINT pay_payment_document_fk1 FOREIGN KEY (owner_id, payment_id) REFERENCES pay_payment(owner_id, id),
    CONSTRAINT pay_payment_document_fk2 FOREIGN KEY (owner_id, activity_id) REFERENCES pay_activity(owner_id, id),
    CONSTRAINT pay_payment_document_fk3 FOREIGN KEY (owner_id, agreement_id) REFERENCES pay_agreement(owner_id, id),
    CONSTRAINT pay_payment_document_fk4 FOREIGN KEY (owner_id, invoice_id)
        REFERENCES pay_invoice(owner_id, id)
        ON DELETE SET NULL,
    CONSTRAINT pay_payment_document_fk5 FOREIGN KEY (owner_id, debit_note_id)
        REFERENCES pay_debit_note(owner_id, id)
        ON DELETE SET NULL,
    CHECK ((invoice_id IS NULL) <> (debit_note_id IS NULL))
);