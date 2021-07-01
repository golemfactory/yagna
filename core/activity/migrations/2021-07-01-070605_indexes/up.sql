create index if not exists activity_natural_id_idx on activity (natural_id);
create index if not exists activity_natural_id_agreement_id_idx on activity (natural_id, agreement_id);
