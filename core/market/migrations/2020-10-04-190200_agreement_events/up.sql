-- Your SQL goes here

CREATE TABLE market_agreement_event(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    agreement_id INTEGER NOT NULL,
    event_type VARCHAR(10) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    issuer VARCHAR(1) NOT NULL,
    reason TEXT,
    signature TEXT,

    FOREIGN KEY(agreement_id) REFERENCES market_agreement (id),
    UNIQUE(agreement_id, event_type)
    CHECK (event_type in ('Terminated', 'Approved', 'Cancelled', 'Rejected'))
    CHECK (issuer in ('P', 'R'))
);

-- Add session_id. Will drop all Agreements.
DROP TABLE market_agreement;
DROP TABLE agreement_state;

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
    state VARCHAR(20) NOT NULL,
    approved_ts DATETIME,

    proposed_signature TEXT,
    approved_signature TEXT,
    committed_signature TEXT,

    CHECK (state in ('Proposal','Pending','Cancelled','Rejected','Approved','Expired','Terminated', 'Approving'))
);

-- Change Proposal state from enum to Text value for better database introspection.
DROP TABLE market_proposal;
DROP TABLE market_proposal_state;

CREATE TABLE market_proposal(
    id VARCHAR(100) NOT NULL PRIMARY KEY,
    prev_proposal_id VARCHAR(100),
    issuer VARCHAR(4) NOT NULL,
    negotiation_id VARCHAR(100) NOT NULL,

    properties TEXT NOT NULL,
    constraints TEXT NOT NULL,

    state VARCHAR(10) NOT NULL,
    creation_ts DATETIME NOT NULL,
    expiration_ts DATETIME NOT NULL,

    FOREIGN KEY(negotiation_id) REFERENCES market_negotiation (id)
    CHECK (state in ('Initial', 'Draft', 'Rejected', 'Accepted', 'Expired'))
    CHECK (issuer in ('Us', 'Them'))
);

-- Rename market_event table. Remove market_event_type and use full text field instead.
DROP TABLE market_event;
DROP TABLE market_event_type;

CREATE TABLE market_negotiation_event(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    subscription_id VARCHAR(100) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    event_type VARCHAR(20) NOT NULL,
    artifact_id VARCHAR(100) NOT NULL,
    reason TEXT,

    CHECK (event_type in ('P-NewProposal', 'P-ProposalRejected', 'P-Agreement', 'P-PropertyQuery', 'R-NewProposal', 'R-ProposalRejected', 'R-PropertyQuery'))
);
