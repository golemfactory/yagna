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
        node_id -> Text,

        insertion_ts -> Nullable<Timestamp>,
        expiration_ts -> Timestamp,
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
    market_event (id) {
        id -> Integer,
        subscription_id -> Text,
        timestamp -> Timestamp,
        event_type -> Integer,
        artifact_id -> Text,
    }
}

table! {
    market_agreement_event_type (event_type) {
        event_type -> Text,
    }
}

table! {
    market_agreement_event (id) {
        id -> Integer,
        agreement_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        issuer -> Text,
        reason -> Nullable<Text>,
        signature -> Nullable<Text>,
    }
}

table! {
    market_proposal_state (id) {
        id -> Integer,
        state -> Text,
    }
}

table! {
    market_proposal (id) {
        id -> Text,
        prev_proposal_id -> Nullable<Text>,

        issuer -> Integer,
        negotiation_id -> Text,

        properties -> Text,
        constraints -> Text,

        state -> Integer,
        creation_ts -> Timestamp,
        expiration_ts -> Timestamp,
    }
}

table! {
    market_negotiation (id) {
        id -> Text,
        subscription_id -> Text,

        offer_id -> Text,
        demand_id -> Text,

        requestor_id -> Text,
        provider_id -> Text,

        agreement_id -> Nullable<Text>,
    }
}

table! {
    market_agreement (id) {
        id -> Text,

        offer_properties -> Text,
        offer_constraints -> Text,

        demand_properties -> Text,
        demand_constraints -> Text,

        offer_id -> Text,
        demand_id -> Text,

        offer_proposal_id -> Text,
        demand_proposal_id -> Text,

        provider_id -> Text,
        requestor_id -> Text,

        session_id -> Nullable<Text>,

        creation_ts -> Timestamp,
        valid_to -> Timestamp,
        approved_date -> Nullable<Timestamp>,
        state -> Integer,

        proposed_signature -> Nullable<Text>,
        approved_signature -> Nullable<Text>,
        committed_signature -> Nullable<Text>,
    }
}

allow_tables_to_appear_in_same_query!(market_demand, market_offer, market_offer_unsubscribed);
allow_tables_to_appear_in_same_query!(market_proposal, market_negotiation);
allow_tables_to_appear_in_same_query!(market_agreement, market_agreement_event);

joinable!(market_agreement_event -> market_agreement (agreement_id));
joinable!(market_negotiation -> market_agreement (agreement_id));
joinable!(market_offer -> market_offer_unsubscribed (id));
joinable!(market_proposal -> market_proposal_state (state));
joinable!(market_proposal -> market_negotiation (negotiation_id));
