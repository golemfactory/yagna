CREATE TABLE "activity" (
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"natural_id" VARCHAR(255) NOT NULL,
	"agreement_id" VARCHAR(255) NOT NULL,
	"state_id" INTEGER NOT NULL,
	"usage_id" INTEGER NOT NULL,
    FOREIGN KEY("state_id") REFERENCES "activity_state" ("id"),
    FOREIGN KEY("usage_id") REFERENCES "activity_usage" ("id"),
    UNIQUE(natural_id)
);

CREATE TABLE "activity_event"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"activity_id" INTEGER NOT NULL,
	"identity_id" VARCHAR(50) NOT NULL,
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

INSERT INTO "activity_event_type"("name") VALUES
    ("CreateActivity"),
    ("DestroyActivity");

