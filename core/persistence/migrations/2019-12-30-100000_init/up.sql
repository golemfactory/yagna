CREATE TABLE "activity"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"natural_id" VARCHAR(255) NOT NULL,
	"agreement_id" INTEGER NOT NULL,
	"state_id" INTEGER NOT NULL,
	"usage_id" INTEGER NOT NULL,
    FOREIGN KEY("state_id") REFERENCES "activity_state" ("id"),
    FOREIGN KEY("usage_id") REFERENCES "activity_usage" ("id"),
    FOREIGN KEY("agreement_id") REFERENCES "agreement" ("id")
);

CREATE TABLE "activity_event"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"activity_id" INTEGER NOT NULL,
	"event_date" DATETIME NOT NULL,
	"event_type_id" INTEGER NOT NULL,
    FOREIGN KEY("activity_id") REFERENCES "activity" ("id"),
    FOREIGN KEY("event_type_id") REFERENCES "activity_event_type" ("id")
);

CREATE TABLE "activity_event_type"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"name" VARCHAR(50) NOT NULL
);

CREATE TABLE "activity_state"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"name" VARCHAR(50) NOT NULL,
	"reason" TEXT,
	"error_message" TEXT,
    "updated_date" DATETIME NOT NULL
);

CREATE TABLE "activity_usage"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"vector_json" TEXT,
    "updated_date" DATETIME NOT NULL
);

CREATE TABLE "agreement"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"natural_id" VARCHAR(255) NOT NULL,
	"state_id" INTEGER NOT NULL,
	"demand_natural_id" VARCHAR(255) NOT NULL,
	"demand_node_id" VARCHAR(255) NOT NULL,
	"demand_properties_json" TEXT NOT NULL,
	"demand_constraints_json" TEXT NOT NULL,
	"offer_natural_id" VARCHAR(255) NOT NULL,
	"offer_node_id" VARCHAR(255) NOT NULL,
	"offer_properties_json" TEXT NOT NULL,
	"offer_constraints_json" TEXT NOT NULL,
	"proposed_signature" TEXT NOT NULL,
	"approved_signature" TEXT NOT NULL,
	"committed_signature" TEXT NOT NULL,
    FOREIGN KEY("state_id") REFERENCES "agreement_state" ("id")
);

CREATE TABLE "agreement_event"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"agreement_id" INTEGER NOT NULL,
	"event_date" DATETIME NOT NULL,
	"event_type_id" INTEGER NOT NULL,
    FOREIGN KEY("agreement_id") REFERENCES "agreement" ("id"),
    FOREIGN KEY("event_type_id") REFERENCES "agreement_event_type" ("id")
);

CREATE TABLE "agreement_event_type"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"name" VARCHAR(50) NOT NULL
);

CREATE TABLE "agreement_state"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"name" VARCHAR(50) NOT NULL
);

CREATE TABLE "allocation"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"natural_id" VARCHAR(255) NOT NULL,
	"created_date" DATETIME NOT NULL,
	"amount" VARCHAR(36) NOT NULL,
	"remaining_amount" VARCHAR(36) NOT NULL,
	"is_deposit" BOOLEAN NOT NULL
);

CREATE TABLE "debit_note"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"natural_id" VARCHAR(255) NOT NULL,
	"agreement_id" INTEGER NOT NULL,
	"state_id" INTEGER NOT NULL,
	"previous_note_id" INTEGER NULL,
	"created_date" DATETIME NOT NULL,
	"activity_id" INTEGER NULL,
	"total_amount_due" VARCHAR(36) NOT NULL,
	"usage_counter_json" TEXT NULL,
	"credit_account" VARCHAR(255) NOT NULL,
	"payment_due_date" DATETIME NULL,
    FOREIGN KEY("activity_id") REFERENCES "activity" ("id"),
    FOREIGN KEY("agreement_id") REFERENCES "agreement" ("id"),
    FOREIGN KEY("state_id") REFERENCES "invoice_debit_note_state" ("id")
);

CREATE TABLE "invoice"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"natural_id" VARCHAR(255) NOT NULL,
	"state_id" INTEGER NOT NULL,
	"last_debit_note_id" INTEGER NULL,
	"created_date" DATETIME NOT NULL,
	"agreement_id" INTEGER NOT NULL,
	"amount" VARCHAR(36) NOT NULL,
	"usage_counter_json" VARCHAR(255) NULL,
	"credit_account" VARCHAR(255) NOT NULL,
	"payment_due_date" DATETIME NOT NULL,
    FOREIGN KEY("agreement_id") REFERENCES "agreement" ("id"),
    FOREIGN KEY("state_id") REFERENCES "invoice_debit_note_state" ("id")
);

CREATE TABLE "invoice_debit_note_state"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"name" VARCHAR(50) NOT NULL
);

CREATE TABLE "invoice_x_activity"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"invoice_id" INTEGER NOT NULL,
	"activity_id" INTEGER NOT NULL,
    FOREIGN KEY("activity_id") REFERENCES "activity" ("id"),
    FOREIGN KEY("invoice_id") REFERENCES "invoice" ("id")
);

CREATE TABLE "payment"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"natural_id" VARCHAR(255) NOT NULL,
	"amount" VARCHAR(36) NOT NULL,
	"debit_account" VARCHAR(255) NOT NULL,
	"created_date" DATETIME NOT NULL
);

CREATE TABLE "payment_x_debit_note"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"payment_id" INTEGER NOT NULL,
	"debit_note_id" INTEGER NOT NULL,
    FOREIGN KEY("debit_note_id") REFERENCES "debit_note" ("id"),
    FOREIGN KEY("payment_id") REFERENCES "payment" ("id")
);

CREATE TABLE "payment_x_invoice"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"payment_id" INTEGER NOT NULL,
	"invoice_id" INTEGER NOT NULL,
    FOREIGN KEY("invoice_id") REFERENCES "invoice" ("id"),
    FOREIGN KEY("payment_id") REFERENCES "payment" ("id")
);

INSERT INTO "activity_event_type"("name") VALUES
    ("CreateActivity"),
    ("DestroyActivity");
