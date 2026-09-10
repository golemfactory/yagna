CREATE TABLE market_agreement_event_old(
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

INSERT INTO market_agreement_event_old(id, agreement_id, event_type, timestamp, issuer, reason, signature)
    SELECT id, agreement_id, event_type, timestamp, issuer, reason, signature
    FROM market_agreement_event
    WHERE event_type != 'TerminationNotice';

DROP TABLE market_agreement_event;
ALTER TABLE market_agreement_event_old RENAME TO market_agreement_event;
