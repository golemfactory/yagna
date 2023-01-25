-- This file should undo anything in `up.sql`

ALTER TABLE app_key RENAME TO _app_key_old;

CREATE TABLE "app_key"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"role_id" INTEGER NOT NULL,
	"name" VARCHAR(255) NOT NULL,
	"key" VARCHAR(255) NOT NULL,
	"identity_id" VARCHAR(255) NOT NULL,
	"created_date" DATETIME NOT NULL,
    FOREIGN KEY("role_id") REFERENCES "role" ("id"),
    FOREIGN KEY (identity_id) REFERENCES identity(identity_id),
    UNIQUE("name")
);

INSERT INTO app_key (id, role_id, name, key, identity_id, created_date)
	SELECT id, role_id, name, key, identity_id, created_date
	FROM _app_key_old;

DROP TABLE IF EXISTS _app_key_old;
