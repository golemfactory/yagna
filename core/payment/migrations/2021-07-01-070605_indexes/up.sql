create index if not exists pay_agreement_owner_idx on pay_agreement (owner_id);
create index if not exists pay_agreement_session_idx on pay_agreement (app_session_id);
create index if not exists pay_agreement_payment_platform_payee_idx on pay_agreement (payment_platform, payee_addr);
create index if not exists pay_agreement_payment_platform_payer_idx on pay_agreement (payment_platform, payer_addr);
create index if not exists pay_allocation_owner_idx on pay_allocation (owner_id);
create index if not exists pay_allocation_timestamp_idx on pay_allocation ("timestamp");
create index if not exists pay_allocation_payment_platform_address_idx on pay_allocation (payment_platform, address);
create index if not exists pay_debit_note_activity_idx on pay_debit_note (activity_id);
create index if not exists pay_debit_note_timestamp_idx on pay_debit_note ("timestamp");
create index if not exists pay_debit_note_activity_owner_idx on pay_debit_note (activity_id, owner_id);
create index if not exists pay_debit_note_event_owner_idx on pay_debit_note_event (owner_id);
create index if not exists pay_debit_note_event_timestamp_idx on pay_debit_note_event ("timestamp");
create index if not exists pay_invoice_timestamp_idx on pay_invoice ("timestamp");
create index if not exists pay_invoice_event_owner_idx on pay_invoice_event (owner_id);
create index if not exists pay_invoice_event_timestamp_idx on pay_invoice_event ("timestamp");
create index if not exists pay_payment_owner_idx on pay_payment (owner_id);
create index if not exists pay_activity_payment_payment_owner_idx on pay_activity_payment (payment_id, owner_id);
create index if not exists pay_agreement_payment_payment_owner_idx on pay_agreement_payment (payment_id, owner_id);
