-- HACK: All this code below is just to drop column network from table payment

PRAGMA foreign_keys=off;

CREATE TABLE transaction_tmp(
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

INSERT INTO transaction_tmp(tx_id, sender, nonce, timestamp, status, tx_type, encoded, signature, tx_hash)
SELECT tx_id, sender, nonce, timestamp, status, tx_type, encoded, signature, tx_hash FROM transaction;


DROP TABLE transaction;

ALTER TABLE transaction_tmp RENAME TO transaction;

PRAGMA foreign_keys=on;
