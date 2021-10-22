PRAGMA foreign_keys=off;

CREATE TABLE `transaction_tmp`(
    tx_id TEXT NOT NULL PRIMARY KEY,
    sender TEXT NOT NULL,
    nonce INTEGER NOT NULL DEFAULT -1,
    status INTEGER NOT NULL,
    tx_type INTEGER NOT NULL,
    tmp_onchain_txs TEXT NULL,
    final_tx TEXT NULL,
    starting_gas_price DOUBLE NULL,
    current_gas_price DOUBLE NULL,
    limit_gas_price DOUBLE NULL,
    final_gas_price DOUBLE NULL,
    final_gas_used INTEGER NULL,
    gas_limit INTEGER NULL,
    time_created DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    time_last_action DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    time_sent DATETIME NULL,
    time_confirmed DATETIME NULL,
    network INTEGER NOT NULL DEFAULT 4,
    last_error_msg TEXT NULL,
    resent_times INT DEFAULT 0,
    signature TEXT NULL,
    encoded TEXT NOT NULL,
    FOREIGN KEY(status) REFERENCES transaction_status (status_id),
    FOREIGN KEY(tx_type) REFERENCES transaction_type (type_id)
);

INSERT INTO `transaction_tmp`(tx_id, sender, nonce, status, tx_type, encoded, signature, final_tx, time_created, time_last_action, network)
SELECT tx_id, sender, nonce, status, tx_type, encoded, signature, tx_hash, timestamp, timestamp, network FROM `transaction`;

DROP TABLE `transaction`;

ALTER TABLE `transaction_tmp` RENAME TO `transaction`;



create index if not exists transaction_tx_hash_idx on "transaction" (final_tx);
create index if not exists transaction_sender_idx on "transaction" (sender);
create index if not exists transaction_status_idx on "transaction" (status);

DELETE FROM `transaction_status`;

INSERT INTO `transaction_status` (status_id, status) VALUES(0, "UNUSED");
INSERT INTO `transaction_status` (status_id, status) VALUES(1, "CREATED");
INSERT INTO `transaction_status` (status_id, status) VALUES(2, "SENT");
INSERT INTO `transaction_status` (status_id, status) VALUES(3, "PENDING");
INSERT INTO `transaction_status` (status_id, status) VALUES(4, "CONFIRMED");
INSERT INTO `transaction_status` (status_id, status) VALUES(10, "ERRORSENT");
INSERT INTO `transaction_status` (status_id, status) VALUES(11, "ERRORONCHAIN");

PRAGMA foreign_keys=on;