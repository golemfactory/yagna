table! {
    activity (id) {
        id -> Integer,
        natural_id -> Text,
        agreement_id -> Text,
        identity_id -> Text,
        state_id -> Integer,
        usage_id -> Integer,
    }
}

table! {
    activity_event (id) {
        id -> Integer,
        activity_id -> Integer,
        identity_id -> Text,
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

joinable!(activity -> activity_state (state_id));
joinable!(activity -> activity_usage (usage_id));
joinable!(activity_event -> activity (activity_id));
joinable!(activity_event -> activity_event_type (event_type_id));

allow_tables_to_appear_in_same_query!(
    activity,
    activity_event,
    activity_event_type,
    activity_state,
    activity_usage,
);
