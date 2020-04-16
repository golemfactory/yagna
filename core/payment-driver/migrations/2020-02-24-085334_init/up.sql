CREATE TABLE "gnt_driver_payment_status"
(
	"status_id" INTEGER NOT NULL PRIMARY KEY,
	"status" VARCHAR(50) NOT NULL
);

INSERT INTO "gnt_driver_payment_status"
	("status_id", "status")
VALUES(1, "REQUESTED");
INSERT INTO "gnt_driver_payment_status"
	("status_id", "status")
VALUES(2, "DONE");
INSERT INTO "gnt_driver_payment_status"
	("status_id", "status")
VALUES(3, "NOT_ENOUGH_FUNDS");
INSERT INTO "gnt_driver_payment_status"
	("status_id", "status")
VALUES(4, "NOT_ENOUGH_GAS");

CREATE TABLE "gnt_driver_transaction"
(
	-- H256 in hex
	"tx_id" VARCHAR(64) NOT NULL PRIMARY KEY,
	"sender" VARCHAR(40) NOT NULL,
	-- U256 in big endian hex
	"nonce" VARCHAR(64) NOT NULL,
	"timestamp" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	"status" INTEGER NOT NULL,
	"encoded" VARCHAR (8000) NOT NULL,
	"signature" VARCHAR (130) NOT NULL,
	"tx_hash" VARCHAR(64)
);

CREATE TABLE "gnt_driver_payment"
(
	"invoice_id" VARCHAR(50) NOT NULL PRIMARY KEY,
	-- U256 in big endian hex
	"amount" VARCHAR(64) NOT NULL,
	-- U256 in big endian hex
	"gas" VARCHAR(64) NOT NULL,
	"recipient" VARCHAR(40) NOT NULL,
	"payment_due_date" DATETIME NOT NULL,
	"status" INTEGER NOT NULL,
	-- H256 in hex
	"tx_id" VARCHAR(64),
	FOREIGN KEY("tx_id") REFERENCES "gnt_driver_transaction" ("tx_id"),
	FOREIGN KEY("status") REFERENCES "gnt_driver_payment_status" ("status_id")
);
