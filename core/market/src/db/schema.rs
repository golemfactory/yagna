table! {
    agreement (id) {
        id -> Integer,
        natural_id -> Text,
        state_id -> Integer,
        demand_node_id -> Text,
        demand_properties_json -> Text,
        demand_constraints -> Text,
        offer_node_id -> Text,
        offer_properties_json -> Text,
        offer_constraints -> Text,
        valid_to -> Timestamp,
        approved_date -> Nullable<Timestamp>,
        proposed_signature -> Text,
        approved_signature -> Text,
        committed_signature -> Nullable<Text>,
    }
}

table! {
    agreement_state (id) {
        id -> Integer,
        name -> Text,
    }
}

//table! {
//    agreement_event (id) {
//        id -> Integer,
//        agreement_id -> Integer,
//        event_date -> Timestamp,
//        event_type_id -> Integer,
//    }
//}
//
//table! {
//    agreement_event_type (id) {
//        id -> Integer,
//        name -> Text,
//    }
//}

joinable!(agreement -> agreement_state (state_id));
//joinable!(agreement_event -> agreement (agreement_id));
//joinable!(agreement_event -> agreement_event_type (event_type_id));

allow_tables_to_appear_in_same_query!(
    agreement,
    agreement_state,
    //agreement_event,
    //agreement_event_type,
);
