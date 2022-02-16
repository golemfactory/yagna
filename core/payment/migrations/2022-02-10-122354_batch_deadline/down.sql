CREATE TABLE pay_batch_order_item_tmp(
    id              VARCHAR (50) NOT NULL,
    payee_addr      VARCHAR(50) NOT NULL,
    amount          VARCHAR(32) NOT NULL,
    driver_order_id VARCHAR(50),
    paid            BOOLEAN NOT NULL DEFAULT FALSE,

    CONSTRAINT PAY_BATCH_ORDER_ITEM_PK PRIMARY KEY (id, payee_addr),
    CONSTRAINT PAY_BATCH_ORDER_ITEM_FK1 FOREIGN KEY (id) REFERENCES pay_batch_order(id)
);

INSERT INTO pay_batch_order_tmp(id, payee_addr, amount, driver_order_id, paid)
SELECT id, payee_addr, amount, driver_order_id, (
  CASE
      WHEN status = "PENDING" THEN FALSE
      ELSE TRUE
  END
) FROM pay_batch_order_item;

DROP TABLE pay_batch_order_item_status;
DROP TABLE pay_batch_order_item;

ALTER TABLE pay_batch_order_item_tmp RENAME TO pay_batch_order_item;

DROP INDEX pay_batch_order_triplet_idx;
