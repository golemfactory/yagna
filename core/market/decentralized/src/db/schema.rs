table! {
    market_agreement (id) {
        id -> Text,
        state_id -> Integer,
        demand_node_id -> Text,
        demand_properties -> Text,
        demand_constraints -> Text,
        offer_node_id -> Text,
        offer_properties -> Text,
        offer_constraints -> Text,
        valid_to -> Timestamp,
        approved_date -> Nullable<Timestamp>,
        proposed_signature -> Text,
        approved_signature -> Text,
        committed_signature -> Nullable<Text>,
    }
}

table! {
    market_agreement_event (id) {
        id -> Integer,
        agreement_id -> Text,
        event_date -> Timestamp,
        event_type_id -> Integer,
    }
}

table! {
    market_agreement_event_type (id) {
        id -> Integer,
        name -> Text,
    }
}

table! {
    market_agreement_state (id) {
        id -> Integer,
        name -> Text,
    }
}

table! {
    market_demand (id) {
        id -> Text,
        properties -> Text,
        constraints -> Text,
        node_id -> Text,
    }
}

table! {
    market_offer (id) {
        id -> Text,
        properties -> Text,
        constraints -> Text,
        node_id -> Text,
    }
}

joinable!(market_agreement -> market_agreement_state (state_id));
joinable!(market_agreement_event -> market_agreement (agreement_id));
joinable!(market_agreement_event -> market_agreement_event_type (event_type_id));

allow_tables_to_appear_in_same_query!(
    market_agreement,
    market_agreement_event,
    market_agreement_event_type,
    market_agreement_state,
    market_demand,
    market_offer,
);
