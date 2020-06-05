-- Your SQL goes here

CREATE TABLE market_offer (
	id VARCHAR(97) NOT NULL PRIMARY KEY,
	properties TEXT NOT NULL,
	constraints TEXT NOT NULL,
	node_id VARCHAR(20) NOT NULL,

	creation_ts DATETIME NOT NULL,
	insertion_ts DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	expiration_ts DATETIME NOT NULL
);

CREATE TABLE market_offer_unsubscribed (
    id VARCHAR(97) NOT NULL PRIMARY KEY,
    timestamp DATETIME NOT NULL,
    node_id VARCHAR(20) NOT NULL
);

CREATE TABLE market_demand (
	id VARCHAR(97) NOT NULL PRIMARY KEY,
	properties TEXT NOT NULL,
	constraints TEXT NOT NULL,
	node_id VARCHAR(20) NOT NULL,

	creation_ts DATETIME NOT NULL,
	insertion_ts DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
	expiration_ts DATETIME NOT NULL
);
