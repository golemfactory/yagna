-- Your SQL goes here
ALTER TABLE pay_agreement ADD created_ts DATETIME;

ALTER TABLE pay_agreement ADD updated_ts DATETIME;

ALTER TABLE pay_activity ADD created_ts DATETIME;

ALTER TABLE pay_activity ADD updated_ts DATETIME;

CREATE INDEX idx_created_ts_pay_agreement ON pay_agreement(created_ts);
CREATE INDEX idx_updated_ts_pay_agreement ON pay_agreement(updated_ts);
CREATE INDEX idx_created_ts_pay_activity ON pay_activity(created_ts);
CREATE INDEX idx_updated_ts_pay_activity ON pay_activity(updated_ts);

DELETE FROM pay_debit_note;

ALTER TABLE pay_debit_note ADD debit_nonce INTEGER NOT NULL;
CREATE UNIQUE INDEX idx_debit_nonce_pay_debit_note ON pay_debit_note(activity_id, debit_nonce);
