create index if not exists market_agreement_offer_proposal_idx on market_agreement (offer_proposal_id);
create index if not exists market_agreement_provider_idx on market_agreement (provider_id);
create index if not exists market_agreement_requestor_idx on market_agreement (requestor_id);
create index if not exists market_agreement_session_idx on market_agreement (session_id);
create index if not exists market_demand_expiration_idx on market_demand (expiration_ts);
create index if not exists market_demand_insertion_idx on market_demand (insertion_ts);
create index if not exists market_offer_expiration_idx on market_offer (expiration_ts);
create index if not exists market_offer_insertion_idx on market_offer (insertion_ts);
create index if not exists market_offer_unsubscribed_expiration_idx on market_offer_unsubscribed (expiration_ts);
create index if not exists market_negotiation_event_subscription_idx on market_negotiation_event (subscription_id);
create index if not exists market_negotiation_event_timestamp_idx on market_negotiation_event ("timestamp");
create index if not exists market_proposal_prev_proposal_idx on market_proposal (prev_proposal_id);
