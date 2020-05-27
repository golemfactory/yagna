-- Your SQL goes here

CREATE TABLE market_offer (
	id VARCHAR(255) NOT NULL PRIMARY KEY,
	properties TEXT NOT NULL,
	constraints TEXT NOT NULL,
	node_id VARCHAR(255) NOT NULL,

	creation_time DATETIME NOT NULL,
	addition_time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	expiration_time DATETIME NOT NULL
);

CREATE TABLE market_demand (
	id VARCHAR(255) NOT NULL PRIMARY KEY,
	properties TEXT NOT NULL,
	constraints TEXT NOT NULL,
	node_id VARCHAR(255) NOT NULL,

	creation_time DATETIME NOT NULL,
	addition_time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	expiration_time DATETIME NOT NULL
);
