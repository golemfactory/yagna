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
	"demand_node_id" VARCHAR(255) NOT NULL,
	"demand_properties_json" TEXT NOT NULL,
	"demand_constraints_json" TEXT NOT NULL,
	"offer_node_id" VARCHAR(255) NOT NULL,
	"offer_properties_json" TEXT NOT NULL,
	"offer_constraints_json" TEXT NOT NULL,
	"proposed_signature" TEXT NOT NULL,
	"approved_signature" TEXT NOT NULL,
	"committed_signature" TEXT,
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
	"id" INTEGER NOT NULL PRIMARY KEY,
	"name" VARCHAR(50) NOT NULL
);

INSERT INTO "activity_event_type"("name") VALUES
    ("CreateActivity"),
    ("DestroyActivity");

INSERT INTO agreement_state(id, name)
values
       (0, 'Proposal'),
       (1, 'Pending'),
       (10, 'Approved'),
       (40, 'Canceled'),
       (41, 'Rejected'),
       (42, 'Expired'),
       (50, 'Terminated');

