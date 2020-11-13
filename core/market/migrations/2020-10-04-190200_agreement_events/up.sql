-- Your SQL goes here

CREATE TABLE market_agreement_event_type(
    event_type VARCHAR(10) NOT NULL PRIMARY KEY
);

INSERT INTO market_agreement_event_type(event_type) VALUES('Terminated');
INSERT INTO market_agreement_event_type(event_type) VALUES('Approved');
INSERT INTO market_agreement_event_type(event_type) VALUES('Cancelled');
INSERT INTO market_agreement_event_type(event_type) VALUES('Rejected');

CREATE TABLE market_agreement_event(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    agreement_id INTEGER NOT NULL,
    event_type VARCHAR(10) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    issuer VARCHAR(1) NOT NULL CHECK (issuer in ('P', 'R')),
    reason TEXT,
    signature TEXT,

    FOREIGN KEY(event_type) REFERENCES market_agreement_event_type (event_type),
    FOREIGN KEY(agreement_id) REFERENCES market_agreement (id),
    UNIQUE(agreement_id, event_type)
);

-- Add session_id. Will drop all Agreements.
DROP TABLE market_agreement;
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

    session_id VARCHAR(100),

    creation_ts DATETIME NOT NULL,
    valid_to DATETIME NOT NULL,
    state INTEGER NOT NULL,
    approved_date DATETIME,

    proposed_signature TEXT,
    approved_signature TEXT,
    committed_signature TEXT,
    FOREIGN KEY(state) REFERENCES agreement_state (id)
);
