-- HACK: All this code below is just to drop column timestamp from table pay_allocation

PRAGMA foreign_keys=off;

CREATE TABLE payment_tmp(
  	order_id VARCHAR(50) NOT NULL PRIMARY KEY,
  	-- U256 in big endian hex
  	amount VARCHAR(64) NOT NULL,
  	-- U256 in big endian hex
  	gas VARCHAR(64) NOT NULL,
  	sender VARCHAR(40) NOT NULL,
  	recipient VARCHAR(40) NOT NULL,
  	payment_due_date DATETIME NOT NULL,
  	status INTEGER NOT NULL,
  	tx_id VARCHAR(128),
  	FOREIGN KEY(tx_id) REFERENCES `transaction` (tx_id),
  	FOREIGN KEY(status) REFERENCES `payment_status` (status_id)
);

INSERT INTO payment_tmp(order_id, amount, gas, sender, recipient, payment_due_date, status, tx_id)
SELECT order_id, amount, gas, sender, recipient, payment_due_date, status, tx_id FROM payment;


DROP TABLE payment;

ALTER TABLE payment_tmp RENAME TO payment;

PRAGMA foreign_keys=on;
