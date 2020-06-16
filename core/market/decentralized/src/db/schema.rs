table! {
    market_demand (id) {
        id -> Text,
        properties -> Text,
        constraints -> Text,
        node_id -> Text,
        creation_ts -> Timestamp,
        insertion_ts -> Nullable<Timestamp>,
        expiration_ts -> Timestamp,
    }
}

table! {
    market_offer (id) {
        id -> Text,
        properties -> Text,
        constraints -> Text,
        node_id -> Text,
        creation_ts -> Timestamp,
        insertion_ts -> Nullable<Timestamp>,
        expiration_ts -> Timestamp,
    }
}

table! {
    market_offer_unsubscribed (id) {
        id -> Text,
        insertion_ts -> Timestamp,
        expiration_ts -> Timestamp,
        node_id -> Text,
    }
}

table! {
    market_event_type (id) {
        id -> Integer,
        event_type -> Text,
        role -> Text,
    }
}

table! {
    market_requestor_event (id) {
        id -> Integer,
        subscription_id -> Text,
        timestamp -> Timestamp,
        event_type -> Integer,
        artifact_id -> Text,
    }
}

table! {
    market_provider_event (id) {
        id -> Integer,
        subscription_id -> Text,
        timestamp -> Timestamp,
        event_type -> Integer,
        artifact_id -> Text,
    }
}

table! {
    market_proposal_state (id) {
        id -> Integer,
        state -> Text,
    }
}

table! {
    market_proposal (proposal_id) {
        proposal_id -> Text,
        prev_proposal_id -> Text,

        negotiation_id -> Integer,

        properties -> Text,
        constraints -> Text,

        state -> Integer,
        creation_ts -> Timestamp,
        expiration_ts -> Timestamp,
    }
}

table! {
    market_negotiation (id) {
        id -> Integer,
        subscription_id -> Text,

        offer_id -> Text,
        demand_id -> Text,

        identity_id -> Text,
        requestor_id -> Text,
        provider_id -> Text,

        agreement_id -> Nullable<Text>,
    }
}

allow_tables_to_appear_in_same_query!(market_demand, market_offer, market_offer_unsubscribed);

joinable!(market_offer -> market_offer_unsubscribed (id));
joinable!(market_proposal -> market_proposal_state (state));
joinable!(market_proposal -> market_negotiation (negotiation_id));
