-- Your SQL goes here
CREATE TABLE pay_batch_order(
    id              VARCHAR (50) NOT NULL,
    ts              DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    owner_id        VARCHAR(50) NOT NULL,
    payer_addr      VARCHAR(50) NOT NULL,
    platform        VARCHAR(50) NOT NULL,
    total_amount    REAL,
    paid            BOOLEAN NOT NULL DEFAULT FALSE,

    CONSTRAINT PAY_BATCH_ORDER_PK PRIMARY KEY(id)
);

CREATE TABLE pay_batch_order_item(
    id              VARCHAR (50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    amount          VARCHAR(32) NOT NULL,
    driver_order_id VARCHAR(50),
    paid            BOOLEAN NOT NULL DEFAULT FALSE,

    CONSTRAINT PAY_BATCH_ORDER_ITEM_PK PRIMARY KEY (id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_FK1 FOREIGN KEY (id) REFERENCES pay_batch_order(id)
);

CREATE TABLE pay_batch_order_item_payment(
    id              VARCHAR (50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    payee_id        VARCHAR(50) NOT NULL,
    json            TEXT NOT NULL,

    CONSTRAINT PAY_BATCH_ORDER_ITEM_PAYMENT_PK PRIMARY KEY (id, payee_addr, payee_id),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_PAYMENT_FK1 FOREIGN KEY (id, payee_addr) REFERENCES pay_batch_order_item(id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_PAYMENT_FK2 FOREIGN KEY (id) REFERENCES pay_batch_order(ID)
);


