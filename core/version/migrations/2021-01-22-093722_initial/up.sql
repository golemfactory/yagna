CREATE TABLE version_release (
	version TEXT PRIMARY KEY,
	name TEXT NOT NULL,
	seen BOOLEAN NOT NULL DEFAULT false,
	release_ts TIMESTAMP NOT NULL,
	insertion_ts TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
	update_ts TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER version_release_update_ts
    AFTER UPDATE
    ON version_release
    FOR EACH ROW
    WHEN NEW.update_ts <= OLD.update_ts    --- this avoids infinite loop
BEGIN
    UPDATE version_release SET update_ts=CURRENT_TIMESTAMP WHERE version=OLD.version;
END;
