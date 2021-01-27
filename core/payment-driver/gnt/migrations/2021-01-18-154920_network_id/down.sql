-- HACK: All this code below is just to drop column network from table gnt_driver_payment

PRAGMA foreign_keys=off;

CREATE TABLE gnt_driver_payment_tmp
(
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
	FOREIGN KEY(tx_id) REFERENCES gnt_driver_transaction (tx_id),
	FOREIGN KEY(status) REFERENCES gnt_driver_payment_status (status_id)
);

INSERT INTO gnt_driver_payment_tmp(order_id, amount, gas, sender, recipient, payment_due_date, status, tx_id)
SELECT order_id, amount, gas, sender, recipient, payment_due_date, status, tx_id FROM gnt_driver_payment;


DROP TABLE gnt_driver_payment;

ALTER TABLE gnt_driver_payment_tmp RENAME TO gnt_driver_payment;

PRAGMA foreign_keys=on;
