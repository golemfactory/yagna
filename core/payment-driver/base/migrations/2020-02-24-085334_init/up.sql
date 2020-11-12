CREATE TABLE `payment_status`
(
	status_id INTEGER NOT NULL PRIMARY KEY,
	status VARCHAR(50) NOT NULL
);

INSERT INTO `payment_status`
	(status_id, status)
VALUES(1, "REQUESTED");
INSERT INTO `payment_status`
	(status_id, status)
VALUES(2, "DONE");
INSERT INTO `payment_status`
	(status_id, status)
VALUES(3, "NOT_ENOUGH_FUNDS");
INSERT INTO `payment_status`
	(status_id, status)
VALUES(4, "NOT_ENOUGH_GAS");
INSERT INTO `payment_status`
	(status_id, status)
VALUES(5, "FAILED");


CREATE TABLE `transaction_status`
(
	status_id INTEGER NOT NULL PRIMARY KEY,
	status VARCHAR(50) NOT NULL
);

INSERT INTO `transaction_status`
	(status_id, status)
VALUES(1, "CREATED");
INSERT INTO `transaction_status`
	(status_id, status)
VALUES(2, "SENT");
INSERT INTO `transaction_status`
	(status_id, status)
VALUES(3, "CONFIRMED");
INSERT INTO `transaction_status`
	(status_id, status)
VALUES(0, "FAILED");


CREATE TABLE transaction_type
(
	type_id INTEGER NOT NULL PRIMARY KEY,
	tx_type VARCHAR(50) NOT NULL
);

INSERT INTO `transaction_type`
	(type_id, tx_type)
VALUES(0, "FAUCET");
INSERT INTO `transaction_type`
	(type_id, tx_type)
VALUES(1, "TRANSFER");

CREATE TABLE `transaction`
(
	tx_id VARCHAR(128) NOT NULL PRIMARY KEY,
	sender VARCHAR(40) NOT NULL,
	-- U256 in big endian hex
	nonce VARCHAR(64) NOT NULL,
	timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	status INTEGER NOT NULL,
	tx_type INTEGER NOT NULL,
	encoded VARCHAR (8000) NOT NULL,
	signature VARCHAR (130) NOT NULL,
	tx_hash VARCHAR(64) NULL UNIQUE,
	FOREIGN KEY(status) REFERENCES transaction_status (status_id),
	FOREIGN KEY(tx_type) REFERENCES transaction_type (type_id)
);

CREATE TABLE `payment`
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
	FOREIGN KEY(tx_id) REFERENCES `transaction` (tx_id),
	FOREIGN KEY(status) REFERENCES `payment_status` (status_id)
);
