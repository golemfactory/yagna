
CREATE TABLE identity(
    identity_id varchar(50) not null primary key, -- //
    key_file_json text not null,
    is_default boolean not null default false,
    is_deleted boolean not null default false,
    alias varchar(50),
    note text,
    "created_date" DATETIME NOT NULL,

    CONSTRAINT id_unk1 UNIQUE (identity_id),
    CONSTRAINT id_unk2 UNIQUE (alias)
);

CREATE TABLE identity_data(
    identity_id varchar(50),
    module_id varchar(50),
    configuration text,
    version integer default 0,
    PRIMARY KEY (identity_id, module_id),
    FOREIGN KEY (identity_id) REFERENCES identity(identity_id)
);

CREATE TABLE "role"(
	"id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	"name" VARCHAR(64) NOT NULL
);

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

INSERT INTO "role"("name") VALUES ("manager");
