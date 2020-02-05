
CREATE TABLE "agreement" (
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


CREATE TABLE "agreement_state" (
	"id" INTEGER NOT NULL PRIMARY KEY,
	"name" VARCHAR(50) NOT NULL
);

INSERT INTO agreement_state(id, name)
values
       (0, 'Proposal'),
       (1, 'Pending'),
       (10, 'Approved'),
       (40, 'Canceled'),
       (41, 'Rejected'),
       (42, 'Expired'),
       (50, 'Terminated');


CREATE TABLE "agreement_event" (
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


-- add foreign key from activity to agreement
--PRAGMA foreign_keys=off;
--BEGIN TRANSACTION;
--
--ALTER TABLE "activity" RENAME TO "_activity_old";

DROP TABLE "activity";

CREATE TABLE "activity" (
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"natural_id" VARCHAR(255) NOT NULL,
	"agreement_id" INTEGER NOT NULL,
	"state_id" INTEGER NOT NULL,
	"usage_id" INTEGER NOT NULL,
    FOREIGN KEY("state_id") REFERENCES "activity_state" ("id"),
    FOREIGN KEY("usage_id") REFERENCES "activity_usage" ("id"),
    FOREIGN KEY("agreement_id") REFERENCES "agreement" ("id")
);

--INSERT INTO "activity" SELECT * FROM "_activity_old";
--
--DROP TABLE "_activity_old";
--
--COMMIT;
--PRAGMA foreign_keys=on;