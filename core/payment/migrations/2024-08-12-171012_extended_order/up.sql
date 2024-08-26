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

    CONSTRAINT PAY_BATCH_ORDER_PK PRIMARY KEY(owner_id, id)
);

CREATE TABLE pay_batch_order_item(
    order_id        VARCHAR(50) NOT NULL,
    owner_id        VARCHAR(50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    amount          VARCHAR(32) NOT NULL,
    payment_id      VARCHAR(50),
    paid            BOOLEAN NOT NULL DEFAULT FALSE,

    CONSTRAINT PAY_BATCH_ORDER_ITEM_PK PRIMARY KEY (owner_id, order_id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_FK1 FOREIGN KEY (owner_id, order_id) REFERENCES pay_batch_order(owner_id, id)
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

    CONSTRAINT PAY_BATCH_ORDER_ITEM_AGREEMENT_PK PRIMARY KEY (owner_id, order_id, payee_addr, agreement_id, activity_id),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_AGREEMENT_FK1 FOREIGN KEY (owner_id, order_id, payee_addr) REFERENCES pay_batch_order_item(owner_id, order_id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_AGREEMENT_FK2 FOREIGN KEY (owner_id, agreement_id) REFERENCES pay_agreement(owner_id, id),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_AGREEMENT_FK3 FOREIGN KEY (owner_id, activity_id) REFERENCES pay_activity(owner_id, id),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_AGREEMENT_FK4 FOREIGN KEY (owner_id, invoice_id)
            REFERENCES pay_invoice(owner_id, id)
            ON DELETE SET NULL,
    CONSTRAINT PAY_BATCH_ORDER_ITEM_AGREEMENT_FK5 FOREIGN KEY (owner_id, debit_note_id)
            REFERENCES pay_debit_note(owner_id, id)
            ON DELETE SET NULL,
    CHECK ((invoice_id IS NULL) <> (debit_note_id IS NULL))
);

CREATE TABLE pay_batch_cycle
(
    owner_id VARCHAR(50) NOT NULL,
    platform VARCHAR(50) NOT NULL,
    created_ts DATETIME NOT NULL,
    updated_ts DATETIME NOT NULL,
    cycle_interval VARCHAR(50),
    cycle_cron VARCHAR(50),
    cycle_last_process DATETIME,
    cycle_next_process DATETIME NOT NULL,
    cycle_max_interval VARCHAR(50) NOT NULL,
    cycle_extra_pay_time VARCHAR(50) NOT NULL,

    CONSTRAINT PAY_BATCH_CYCLE_PK PRIMARY KEY(owner_id, platform),
    CONSTRAINT PAY_BATCH_CYCLE_CHECK_1 CHECK((cycle_interval IS NULL) <> (cycle_cron IS NULL))
);

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
    created_ts       DATETIME NOT NULL,
    updated_ts       DATETIME NOT NULL,
    timeout          DATETIME NOT NULL,
    released         BOOLEAN NOT NULL,
    deposit          TEXT,

    CONSTRAINT PAY_ALLOCATION_PK PRIMARY KEY(owner_id, id)
);

CREATE INDEX pay_allocation_payment_platform_address_idx ON pay_allocation (payment_platform, address);
CREATE INDEX pay_allocation_timestamp_idx ON pay_allocation (timestamp);

CREATE TABLE pay_allocation_document
(
    owner_id      VARCHAR(50) NOT NULL,
    allocation_id VARCHAR(50) NOT NULL,
    agreement_id  VARCHAR(50) NOT NULL,
    invoice_id    VARCHAR(50),
    activity_id   VARCHAR(50),
    debit_note_id VARCHAR(50),
    spent_amount  VARCHAR(32) NOT NULL,

    CONSTRAINT PAY_ALLOCATION_DOCUMENT_PK PRIMARY KEY (owner_id, allocation_id, agreement_id, activity_id),
    CONSTRAINT PAY_ALLOCATION_DOCUMENT_FK1 FOREIGN KEY (owner_id, allocation_id) REFERENCES pay_allocation(owner_id, id),
    CONSTRAINT PAY_ALLOCATION_DOCUMENT_FK2 FOREIGN KEY (owner_id, activity_id) REFERENCES pay_activity(owner_id, id),
    CONSTRAINT PAY_ALLOCATION_DOCUMENT_FK3 FOREIGN KEY (owner_id, agreement_id) REFERENCES pay_agreement(owner_id, id),
    CONSTRAINT PAY_ALLOCATION_DOCUMENT_FK4 FOREIGN KEY (owner_id, invoice_id)
        REFERENCES pay_invoice(owner_id, id)
        ON DELETE SET NULL,
    CONSTRAINT PAY_ALLOCATION_DOCUMENT_FK5 FOREIGN KEY (owner_id, debit_note_id)
        REFERENCES pay_debit_note(owner_id, id)
        ON DELETE SET NULL,
    CHECK ((invoice_id IS NULL) <> (debit_note_id IS NULL))
);

