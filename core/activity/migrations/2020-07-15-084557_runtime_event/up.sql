CREATE TABLE "runtime_event" (
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "activity_id" INTEGER NOT NULL,
    "batch_id" VARCHAR(255) NOT NULL,
    "index" INTEGER NOT NULL,
    "timestamp" DATETIME NOT NULL,
    "type_id" INTEGER NOT NULL,
    "command" TEXT,
    "return_code" INTEGER,
    "message" TEXT,
    FOREIGN KEY("activity_id") REFERENCES "activity" ("id"),
    FOREIGN KEY("type_id") REFERENCES "runtime_event_type" ("id")
);

CREATE TABLE "runtime_event_type"(
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "name" VARCHAR(50) NOT NULL
);

INSERT INTO "runtime_event_type"("name") VALUES
    ("Started"),
    ("Finished"),
    ("StdOut"),
    ("StdErr");
