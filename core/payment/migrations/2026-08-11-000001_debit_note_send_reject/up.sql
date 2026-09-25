ALTER TABLE pay_debit_note ADD COLUMN send_reject BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX pay_debit_note_send_reject_idx ON pay_debit_note (send_reject);
