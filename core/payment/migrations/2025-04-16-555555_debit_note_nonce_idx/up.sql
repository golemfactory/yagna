DROP INDEX idx_debit_nonce_pay_debit_note;
CREATE UNIQUE INDEX idx_debit_nonce_pay_debit_note ON pay_debit_note(role, activity_id, debit_nonce);
