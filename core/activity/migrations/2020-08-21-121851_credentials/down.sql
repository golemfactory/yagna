CREATE TABLE activity_event_migrate (
	id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	activity_id INTEGER NOT NULL,
	identity_id VARCHAR(50) NOT NULL,
	event_date DATETIME NOT NULL,
	event_type_id INTEGER NOT NULL,
    FOREIGN KEY(activity_id) REFERENCES activity (id),
    FOREIGN KEY(event_type_id) REFERENCES activity_event_type (id)
);

INSERT INTO activity_event_migrate(activity_id, identity_id, event_date, event_type_id)
SELECT activity_id, identity_id, event_date, event_type_id
FROM activity_event;

DROP TABLE activity_event;
ALTER TABLE activity_event_migrate RENAME TO activity_event;

DROP TABLE activity_credentials;
