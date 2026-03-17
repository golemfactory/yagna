-- Your SQL goes here
DROP TABLE market_offer;
CREATE TABLE market_offer (
    id VARCHAR(97) NOT NULL PRIMARY KEY,
    properties TEXT NOT NULL,
    constraints TEXT NOT NULL,
    node_id VARCHAR(20) NOT NULL,
    owned BOOLEAN NOT NULL DEFAULT(FALSE),

    creation_ts DATETIME NOT NULL,
    insertion_ts DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    expiration_ts DATETIME NOT NULL
);