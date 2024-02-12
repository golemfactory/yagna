ALTER TABLE pay_invoice ADD COLUMN send_reject BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX pay_invoice_send_reject_idx ON pay_invoice (send_reject);
