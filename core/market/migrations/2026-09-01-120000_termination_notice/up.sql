-- Allow the TerminationNotice Agreement event and store its deadline in a
-- typed column. SQLite can't alter CHECK constraints, so the table is rebuilt.
-- UNIQUE(agreement_id, event_type) already guarantees at most one notice per
-- Agreement.

CREATE TABLE market_agreement_event_new(
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    agreement_id INTEGER NOT NULL,
    event_type VARCHAR(20) NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    issuer VARCHAR(1) NOT NULL,
    reason TEXT,
    signature TEXT,
    termination_deadline DATETIME,

    FOREIGN KEY(agreement_id) REFERENCES market_agreement (id),
    UNIQUE(agreement_id, event_type)
    CHECK (event_type in ('Terminated', 'Approved', 'Cancelled', 'Rejected', 'TerminationNotice'))
    CHECK (issuer in ('P', 'R'))
);

INSERT INTO market_agreement_event_new(id, agreement_id, event_type, timestamp, issuer, reason, signature)
    SELECT id, agreement_id, event_type, timestamp, issuer, reason, signature
    FROM market_agreement_event;

DROP TABLE market_agreement_event;
ALTER TABLE market_agreement_event_new RENAME TO market_agreement_event;
