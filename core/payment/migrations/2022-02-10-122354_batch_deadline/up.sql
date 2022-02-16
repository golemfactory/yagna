CREATE TABLE pay_batch_order_item_status(
    status VARCHAR(50) NOT NULL PRIMARY KEY
);

INSERT INTO pay_batch_order_item_status(status) VALUES('PENDING');
INSERT INTO pay_batch_order_item_status(status) VALUES('SENT');
INSERT INTO pay_batch_order_item_status(status) VALUES('PAID');

CREATE TABLE pay_batch_order_item_tmp (
    id                  VARCHAR(50) NOT NULL,
    payee_addr          VARCHAR(50) NOT NULL,
    amount              VARCHAR(32) NOT NULL,
    driver_order_id     VARCHAR(50),
    status              VARCHAR(50) NOT NULL DEFAULT "PENDING",
    payment_due_date    DATETIME,

    CONSTRAINT PAY_BATCH_ORDER_ITEM_PK  PRIMARY KEY (id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_FK1 FOREIGN KEY (id) REFERENCES pay_batch_order (id)
    CONSTRAINT PAY_BATCH_ORDER_ITEM_FK2 FOREIGN KEY (status) REFERENCES pay_batch_order_item_status (status)
);

INSERT INTO pay_batch_order_item_tmp(id, payee_addr, amount, driver_order_id, status)
SELECT id, payee_addr, amount, driver_order_id, (
  CASE
    WHEN paid IS TRUE THEN "PAID"
    ELSE "PENDING"
  END
) FROM pay_batch_order_item;

DROP TABLE pay_batch_order_item;
ALTER TABLE pay_batch_order_item_tmp RENAME TO pay_batch_order_item;

CREATE INDEX IF NOT EXISTS pay_batch_order_triplet_idx on pay_batch_order (owner_id, payer_addr, platform);
