ALTER TABLE activity_event ADD COLUMN app_session_id VARCHAR(100);

CREATE INDEX idx_app_session_id
ON activity_event(app_session_id);
