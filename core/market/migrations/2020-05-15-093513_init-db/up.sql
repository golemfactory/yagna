-- Your SQL goes here

CREATE TABLE market_offer (
    id VARCHAR(97) NOT NULL PRIMARY KEY,
    properties TEXT NOT NULL,
    constraints TEXT NOT NULL,
    node_id VARCHAR(20) NOT NULL,

    creation_ts DATETIME NOT NULL,
    insertion_ts DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    expiration_ts DATETIME NOT NULL
);

CREATE TABLE market_offer_unsubscribed (
    id VARCHAR(97) NOT NULL PRIMARY KEY,
    node_id VARCHAR(20) NOT NULL,

    insertion_ts DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    expiration_ts DATETIME NOT NULL
);

CREATE TABLE market_demand (
    id VARCHAR(97) NOT NULL PRIMARY KEY,
    properties TEXT NOT NULL,
    constraints TEXT NOT NULL,
    node_id VARCHAR(20) NOT NULL,

    creation_ts DATETIME NOT NULL,
    insertion_ts DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    expiration_ts DATETIME NOT NULL
);

CREATE TABLE market_event_type(
    id INTEGER NOT NULL PRIMARY KEY,
    event_type VARCHAR(50) NOT NULL,
    role VARCHAR(10) NOT NULL CHECK (role in ('Requestor', 'Provider'))
);

INSERT INTO market_event_type(id, event_type, role) VALUES
    (1001, "Proposal", "Provider"),
    (1002, "Agreement", "Provider"),
    (1003, "PropertyQuery", "Provider"),
    (2001, "Proposal", "Requestor"),
    (2002, "PropertyQuery", "Requestor");

CREATE TABLE market_event(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    subscription_id INTEGER NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    event_type INTEGER NOT NULL,
    artifact_id VARCHAR(100) NOT NULL,

    FOREIGN KEY(event_type) REFERENCES market_event_type (id)
);

CREATE TABLE market_proposal_state(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    state VARCHAR(10) NOT NULL UNIQUE
);

INSERT INTO market_proposal_state(id, state) VALUES
    (0, "Initial"),
    (1, "Draft"),
    (2, "Rejected"),
    (3, "Accepted"),
    (4, "Expired");

CREATE TABLE market_negotiation(
    id VARCHAR(100) NOT NULL PRIMARY KEY,
    subscription_id VARCHAR(97) NOT NULL,

    offer_id VARCHAR(97) NOT NULL,
    demand_id VARCHAR(97) NOT NULL,

    requestor_id VARCHAR(20) NOT NULL,
    provider_id VARCHAR(20) NOT NULL,

    agreement_id VARCHAR(100)
);

CREATE TABLE market_proposal(
    id VARCHAR(100) NOT NULL PRIMARY KEY,
    prev_proposal_id VARCHAR(100),
    issuer INTEGER NOT NULL,
    negotiation_id VARCHAR(100) NOT NULL,

    properties TEXT NOT NULL,
    constraints TEXT NOT NULL,

    state INTEGER NOT NULL,
    creation_ts DATETIME NOT NULL,
    expiration_ts DATETIME NOT NULL,

    FOREIGN KEY(state) REFERENCES market_proposal_state (id),
    FOREIGN KEY(negotiation_id) REFERENCES market_negotiation (id)
);

CREATE TABLE market_agreement(
    id VARCHAR(100) NOT NULL PRIMARY KEY,

    demand_properties TEXT NOT NULL,
    demand_constraints TEXT NOT NULL,

    offer_properties TEXT NOT NULL,
    offer_constraints TEXT NOT NULL,

    offer_id VARCHAR(97) NOT NULL,
    demand_id VARCHAR(97) NOT NULL,

    offer_proposal_id VARCHAR(100) NOT NULL,
    demand_proposal_id VARCHAR(100) NOT NULL,

    provider_id VARCHAR(20) NOT NULL,
    requestor_id VARCHAR(20) NOT NULL,

    creation_ts DATETIME NOT NULL,
    valid_to DATETIME NOT NULL,
    state INTEGER NOT NULL,
    approved_date DATETIME,

    proposed_signature TEXT,
    approved_signature TEXT,
    committed_signature TEXT,
    FOREIGN KEY(state) REFERENCES agreement_state (id)
);


CREATE TABLE agreement_state(
    id INTEGER NOT NULL PRIMARY KEY,
    name VARCHAR(50) NOT NULL
);

INSERT INTO agreement_state(id, name)
values
    (0, 'Proposal'),
    (1, 'Pending'),
    (2, 'Cancelled'),
    (3, 'Rejected'),
    (4, 'Approved'),
    (5, 'Expired'),
    (6, 'Terminated');
