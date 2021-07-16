create index if not exists payment_sender_idx on payment (sender);
create index if not exists payment_tx_idx on payment (tx_id);
create index if not exists transaction_tx_hash_idx on "transaction" (tx_hash);
create index if not exists transaction_sender_idx on "transaction" (sender);
create index if not exists transaction_status_idx on "transaction" (status);
