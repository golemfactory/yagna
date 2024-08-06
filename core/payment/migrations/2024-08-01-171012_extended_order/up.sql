-- Your SQL goes here
CREATE TABLE pay_batch_order(
    id              VARCHAR (50) NOT NULL,
    ts              DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    owner_id        VARCHAR(50) NOT NULL,
    payer_addr      VARCHAR(50) NOT NULL,
    driver          VARCHAR(50) NOT NULL,
    platform        VARCHAR(50) NOT NULL,
    total_amount    VARCHAR(32) NOT NULL,
    paid            BOOLEAN NOT NULL DEFAULT FALSE,

    CONSTRAINT PAY_BATCH_ORDER_PK PRIMARY KEY(owner_id, id)
);

CREATE TABLE pay_batch_order_item(
    order_id        VARCHAR(50) NOT NULL,
    owner_id        VARCHAR(50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    amount          VARCHAR(32) NOT NULL,
    driver_order_id VARCHAR(50),
    paid            BOOLEAN NOT NULL DEFAULT FALSE,

    CONSTRAINT PAY_BATCH_ORDER_ITEM_PK PRIMARY KEY (owner_id, order_id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_FK1 FOREIGN KEY (owner_id, order_id) REFERENCES pay_batch_order(owner_id, id)
);

CREATE TABLE pay_batch_order_item_agreement(
    order_id        VARCHAR(50) NOT NULL,
    owner_id        VARCHAR(50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    agreement_id    VARCHAR(50) NOT NULL,
    invoice_id      VARCHAR(50) NULL,
    amount          VARCHAR(32) NOT NULL,

    CONSTRAINT PAY_BATCH_ORDER_ITEM_INVOICE_PK PRIMARY KEY (owner_id, order_id, agreement_id),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_INVOICE_FK1 FOREIGN KEY (owner_id, order_id, payee_addr) REFERENCES pay_batch_order_item(owner_id, order_id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_INVOICE_FK2 FOREIGN KEY (owner_id, agreement_id) REFERENCES pay_agreement(owner_id, id)
);

CREATE TABLE pay_batch_order_item_activity(
    order_id        VARCHAR(50) NOT NULL,
    owner_id        VARCHAR(50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    activity_id     VARCHAR(50) NOT NULL,
    debit_note_id   VARCHAR(50) NULL,
    amount          VARCHAR(32) NOT NULL,

    CONSTRAINT PAY_BATCH_ORDER_ITEM_DEBIT_NOTE_PK PRIMARY KEY (owner_id, order_id, activity_id),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_DEBIT_NOTE_FK1 FOREIGN KEY (owner_id, order_id, payee_addr) REFERENCES pay_batch_order_item(owner_id, order_id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_DEBIT_NOTE_FK2 FOREIGN KEY (owner_id, activity_id) REFERENCES pay_activity(owner_id, id)
);

CREATE TABLE pay_batch_order_item_payment(
    order_id        VARCHAR (50) NOT NULL,
    owner_id        VARCHAR(50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    payee_id        VARCHAR(50) NOT NULL,
    json            TEXT NOT NULL,

    CONSTRAINT PAY_BATCH_ORDER_ITEM_PAYMENT_FK1 FOREIGN KEY (owner_id, order_id, payee_addr) REFERENCES pay_batch_order_item(owner_id, order_id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_PAYMENT_FK2 FOREIGN KEY (owner_id, order_id) REFERENCES pay_batch_order(owner_id, id)
);


