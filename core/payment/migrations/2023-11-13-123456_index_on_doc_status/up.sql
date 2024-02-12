create index if not exists pay_invoice_status on pay_invoice ("status");
create index if not exists pay_debit_note_status on pay_debit_note ("status");
create index if not exists pay_debit_note_due_date on pay_debit_note (payment_due_date);
