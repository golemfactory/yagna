DROP INDEX pay_debit_note_send_reject_idx;

ALTER TABLE pay_debit_note DROP COLUMN send_reject;
