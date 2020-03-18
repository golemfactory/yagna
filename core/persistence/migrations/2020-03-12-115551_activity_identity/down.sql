DROP TABLE activity;

CREATE TABLE activity (
	id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	natural_id VARCHAR(255) NOT NULL,
	agreement_id VARCHAR(255) NOT NULL,
	state_id INTEGER NOT NULL,
	usage_id INTEGER NOT NULL,
    FOREIGN KEY(state_id) REFERENCES activity_state (id),
    FOREIGN KEY(usage_id) REFERENCES activity_usage (id)
);

DROP TABLE activity_event;

CREATE TABLE activity_event(
	id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	activity_id INTEGER NOT NULL,
	event_date DATETIME NOT NULL,
	event_type_id INTEGER NOT NULL,
    FOREIGN KEY(activity_id) REFERENCES activity (id),
    FOREIGN KEY(event_type_id) REFERENCES activity_event_type (id)
);
