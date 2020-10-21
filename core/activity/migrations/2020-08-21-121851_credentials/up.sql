ALTER TABLE activity_event ADD COLUMN requestor_pub_key BLOB;

CREATE TABLE activity_credentials (
	activity_id TEXT NOT NULL PRIMARY KEY,
	credentials TEXT NOT NULL,
    UNIQUE(activity_id)
);
