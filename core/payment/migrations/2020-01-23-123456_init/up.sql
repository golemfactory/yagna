CREATE TABLE "invoice_status"(
    "status" VARCHAR(50) NOT NULL PRIMARY KEY
);

INSERT INTO "invoice_status"("status") VALUES("ISSUED");
INSERT INTO "invoice_status"("status") VALUES("RECEIVED");
INSERT INTO "invoice_status"("status") VALUES("ACCEPTED");
INSERT INTO "invoice_status"("status") VALUES("REJECTED");
INSERT INTO "invoice_status"("status") VALUES("FAILED");
INSERT INTO "invoice_status"("status") VALUES("SETTLED");
INSERT INTO "invoice_status"("status") VALUES("CANCELLED");


CREATE TABLE "debit_note"(
	"id" VARCHAR(50) NOT NULL PRIMARY KEY,
	"previous_debit_note_id" VARCHAR(50) NULL,
	"agreement_id" VARCHAR(50) NOT NULL,
	"activity_id" VARCHAR(50) NULL,
	"status" VARCHAR(50) NOT NULL,
	"timestamp" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	"total_amount_due" INTEGER NOT NULL,
	"usage_counter_vector" BLOB NULL,
	"credit_account_id" VARCHAR(50) NOT NULL,
	"payment_platform" VARCHAR(50) NULL,
	"payment_due_date" DATETIME NULL,
	FOREIGN KEY("previous_debit_note_id") REFERENCES "debit_note" ("id"),
    FOREIGN KEY("status") REFERENCES "invoice_status" ("status")
);

CREATE TABLE "invoice"(
	"id" VARCHAR(50) NOT NULL PRIMARY KEY,
	"last_debit_note_id" VARCHAR(50) NOT NULL,
	"agreement_id" VARCHAR(50) NOT NULL,
	"status" VARCHAR(50) NOT NULL,
	"timestamp" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	"amount" VARCHAR(36) NOT NULL,
	"usage_counter_vector" BLOB NULL,
	"credit_account_id" VARCHAR(50) NOT NULL,
	"payment_platform" VARCHAR(50) NULL,
	"payment_due_date" DATETIME NOT NULL,
	FOREIGN KEY("last_debit_note_id") REFERENCES "debit_note" ("id"),
    FOREIGN KEY("status") REFERENCES "invoice_status" ("status")
);

CREATE TABLE "invoice_x_activity"(
	"invoice_id" VARCHAR(50) NOT NULL,
	"activity_id" VARCHAR(50) NOT NULL,
	PRIMARY KEY("invoice_id", "activity_id"),
    FOREIGN KEY("invoice_id") REFERENCES "invoice" ("id")
);

CREATE TABLE "allocation"(
	"id" VARCHAR(50) NOT NULL PRIMARY KEY,
	"total_amount" INTEGER NOT NULL,
	"timeout" DATETIME NOT NULL,
	"make_deposit" BOOLEAN NOT NULL
);

CREATE TABLE "payment"(
	"id" VARCHAR(50) NOT NULL PRIMARY KEY,
	"amount" INTEGER NOT NULL,
	"timestamp" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	"allocation_id" VARCHAR(50) NULL,
	"details" TEXT NOT NULL,
	FOREIGN KEY("allocation_id") REFERENCES "allocation" ("id")
);

CREATE TABLE "payment_x_debit_note"(
	"payment_id" VARCHAR(50) NOT NULL,
	"debit_note_id" VARCHAR(50) NOT NULL,
	PRIMARY KEY("payment_id", "debit_note_id"),
    FOREIGN KEY("payment_id") REFERENCES "payment" ("id"),
    FOREIGN KEY("debit_note_id") REFERENCES "debit_note" ("id")
);

CREATE TABLE "payment_x_invoice"(
	"payment_id" VARCHAR(50) NOT NULL,
	"invoice_id" VARCHAR(50) NOT NULL,
	PRIMARY KEY("payment_id", "invoice_id"),
    FOREIGN KEY("payment_id") REFERENCES "payment" ("id"),
    FOREIGN KEY("invoice_id") REFERENCES "invoice" ("id")
);

CREATE TABLE "invoice_event_type"(
    "event_type" VARCHAR(50) NOT NULL PRIMARY KEY
);

INSERT INTO "invoice_event_type"("event_type") VALUES("RECEIVED");
INSERT INTO "invoice_event_type"("event_type") VALUES("ACCEPTED");
INSERT INTO "invoice_event_type"("event_type") VALUES("REJECTED");
INSERT INTO "invoice_event_type"("event_type") VALUES("CANCELLED");

CREATE TABLE "debit_note_event"(
    "debit_note_id" VARCHAR(50) NOT NULL,
    "event_type" VARCHAR(50) NOT NULL,
	"timestamp" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	"details" TEXT NULL,
	PRIMARY KEY("debit_note_id", "event_type"),
	FOREIGN KEY("debit_note_id") REFERENCES "debit_note" ("id"),
	FOREIGN KEY("event_type") REFERENCES "invoice_event_type" ("event_type")
);

CREATE TABLE "invoice_event"(
    "invoice_id" VARCHAR(50) NOT NULL,
    "event_type" VARCHAR(50) NOT NULL,
	"timestamp" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	"details" TEXT NULL,
	PRIMARY KEY("invoice_id", "event_type"),
	FOREIGN KEY("invoice_id") REFERENCES "invoice" ("id"),
	FOREIGN KEY("event_type") REFERENCES "invoice_event_type" ("event_type")
);
