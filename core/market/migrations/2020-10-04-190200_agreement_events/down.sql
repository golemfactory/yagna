-- This file should undo anything in `up.sql`

DROP TABLE market_agreement_event;
DROP TABLE market_agreement_event_type;

-- Restore Agreement without session_id
-- Will drop all existing Agreements
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

    creation_ts DATETIME NOT NULL,
    valid_to DATETIME NOT NULL,
    state INTEGER NOT NULL,
    approved_date DATETIME,

    proposed_signature TEXT,
    approved_signature TEXT,
    committed_signature TEXT,
    FOREIGN KEY(state) REFERENCES agreement_state (id)
);
