
-- drop foreign key from activity to agreement and change agreement_id column type

--ALTER TABLE "activity" RENAME TO "_activity_old";

DROP TABLE "activity";

CREATE TABLE "activity" (
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"natural_id" VARCHAR(255) NOT NULL,
	"agreement_id" VARCHAR(255) NOT NULL,
	"state_id" INTEGER NOT NULL,
	"usage_id" INTEGER NOT NULL,
    FOREIGN KEY("state_id") REFERENCES "activity_state" ("id"),
    FOREIGN KEY("usage_id") REFERENCES "activity_usage" ("id")
);

--INSERT INTO "activity"
--    SELECT
--        "activity"."id",
--        "activity"."natural_id",
--        "agreement"."natural_id",
--        "activity"."state_id",
--        "activity"."usage_id"
--    FROM
--        "agreement", "_activity_old" as "activity"
--    WHERE
--        "activity"."agreement_id" = "agreement"."id";
--
--DROP TABLE "_activity_old";

DROP TABLE "agreement";

DROP TABLE "agreement_state";

DROP TABLE "agreement_event";

DROP TABLE "agreement_event_type";

