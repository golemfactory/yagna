-- Your SQL goes here

CREATE TABLE market_agreement (
	id VARCHAR(255) NOT NULL PRIMARY KEY,
	state_id INTEGER NOT NULL,
	demand_node_id VARCHAR(255) NOT NULL,
	demand_properties TEXT NOT NULL,
	demand_constraints TEXT NOT NULL,
	offer_node_id VARCHAR(255) NOT NULL,
	offer_properties TEXT NOT NULL,
	offer_constraints TEXT NOT NULL,
	valid_to DATETIME NOT NULL,
	approved_date DATETIME,
	proposed_signature TEXT NOT NULL,
	approved_signature TEXT NOT NULL,
	committed_signature TEXT,
    FOREIGN KEY(state_id) REFERENCES market_agreement_state (id)
);


CREATE TABLE market_agreement_state(
	id INTEGER NOT NULL PRIMARY KEY,
	name VARCHAR(50) NOT NULL
);

INSERT INTO market_agreement_state(id, name)
values
       (0, 'Proposal'),
       (1, 'Pending'),
       (10, 'Approved'),
       (40, 'Canceled'),
       (41, 'Rejected'),
       (42, 'Expired'),
       (50, 'Terminated');


CREATE TABLE market_agreement_event(
	id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	agreement_id VARCHAR(255) NOT NULL,
	event_date DATETIME NOT NULL,
	event_type_id INTEGER NOT NULL,
    FOREIGN KEY(agreement_id) REFERENCES market_agreement (id),
    FOREIGN KEY(event_type_id) REFERENCES market_agreement_event_type (id)
);

CREATE TABLE market_agreement_event_type(
	id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	name VARCHAR(50) NOT NULL
);

CREATE TABLE market_offer (
	id VARCHAR(255) NOT NULL PRIMARY KEY,
	properties TEXT NOT NULL,
	constraints TEXT NOT NULL,
	node_id VARCHAR(255) NOT NULL
);

CREATE TABLE market_demand (
	id VARCHAR(255) NOT NULL PRIMARY KEY,
	properties TEXT NOT NULL,
	constraints TEXT NOT NULL,
	node_id VARCHAR(255) NOT NULL
);
