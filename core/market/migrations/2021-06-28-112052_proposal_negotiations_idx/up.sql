create index if not exists market_proposal_expiration_idx on market_proposal (expiration_ts);
create index if not exists market_proposal_negotiation_idx on market_proposal (expiration_ts, negotiation_id);
