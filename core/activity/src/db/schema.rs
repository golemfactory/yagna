table! {
    activity (id) {
        id -> Integer,
        natural_id -> Text,
        agreement_id -> Text,
        state_id -> Integer,
        usage_id -> Integer,
    }
}

table! {
    activity_credentials (activity_id) {
        activity_id -> Text,
        credentials -> Text,
    }
}

table! {
    activity_event (id) {
        id -> Integer,
        activity_id -> Integer,
        identity_id -> Text,
        app_session_id -> Nullable<Text>,
        event_date -> Timestamp,
        event_type_id -> Integer,
        requestor_pub_key -> Nullable<Binary>,
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
    runtime_event (id) {
        id -> Integer,
        activity_id -> Integer,
        batch_id -> Text,
        index -> Integer,
        timestamp -> Timestamp,
        type_id -> Integer,
        command -> Nullable<Text>,
        return_code -> Nullable<Integer>,
        message -> Nullable<Text>,
    }
}

table! {
    runtime_event_type (id) {
        id -> Integer,
        name -> Text,
    }
}

joinable!(activity -> activity_state (state_id));
joinable!(activity -> activity_usage (usage_id));
joinable!(activity_event -> activity (activity_id));
joinable!(activity_event -> activity_event_type (event_type_id));
joinable!(runtime_event -> activity (activity_id));
joinable!(runtime_event -> runtime_event_type (type_id));

allow_tables_to_appear_in_same_query!(
    activity,
    activity_credentials,
    activity_event,
    activity_event_type,
    activity_state,
    activity_usage,
    runtime_event,
    runtime_event_type,
);
