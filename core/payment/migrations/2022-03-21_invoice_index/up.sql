-- Used in subselect from .(incoming|outgoing)_transaction_summary()

create index if not exists pay_invoice_agreement_id_timestamp_idx on pay_invoice (agreement_id, "timestamp");
