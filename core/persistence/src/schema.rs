table! {
    activity (id) {
        id -> Integer,
        natural_id -> Text,
        agreement_id -> Integer,
        state_id -> Integer,
        usage_id -> Integer,
    }
}

table! {
    activity_event (id) {
        id -> Integer,
        activity_id -> Integer,
        event_date -> Timestamp,
        event_type_id -> Integer,
    }
}

table! {
    activity_event_type (id) {
        id -> Integer,
        name -> Text,
    }
}

table! {
    activity_state (id) {
        id -> Integer,
        name -> Text,
        reason -> Nullable<Text>,
        error_message -> Nullable<Text>,
        updated_date -> Timestamp,
    }
}

table! {
    activity_usage (id) {
        id -> Integer,
        vector_json -> Nullable<Text>,
        updated_date -> Timestamp,
    }
}

table! {
    agreement (id) {
        id -> Integer,
        natural_id -> Text,
        state_id -> Integer,
        demand_node_id -> Text,
        demand_properties_json -> Text,
        demand_constraints_json -> Text,
        offer_node_id -> Text,
        offer_properties_json -> Text,
        offer_constraints_json -> Text,
        proposed_signature -> Text,
        approved_signature -> Text,
        committed_signature -> Nullable<Text>,
    }
}

table! {
    agreement_event (id) {
        id -> Integer,
        agreement_id -> Integer,
        event_date -> Timestamp,
        event_type_id -> Integer,
    }
}

table! {
    agreement_event_type (id) {
        id -> Integer,
        name -> Text,
    }
}

table! {
    agreement_state (id) {
        id -> Integer,
        name -> Text,
    }
}

joinable!(activity -> activity_state (state_id));
joinable!(activity -> activity_usage (usage_id));
joinable!(activity -> agreement (agreement_id));
joinable!(activity_event -> activity (activity_id));
joinable!(activity_event -> activity_event_type (event_type_id));
joinable!(agreement -> agreement_state (state_id));
joinable!(agreement_event -> agreement (agreement_id));
joinable!(agreement_event -> agreement_event_type (event_type_id));

allow_tables_to_appear_in_same_query!(
    activity,
    activity_event,
    activity_event_type,
    activity_state,
    activity_usage,
    agreement,
    agreement_event,
    agreement_event_type,
    agreement_state,
);
