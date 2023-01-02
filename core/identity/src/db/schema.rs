// @generated automatically by Diesel CLI.

diesel::table! {
    app_key (id) {
        id -> Integer,
        role_id -> Integer,
        name -> Text,
        key -> Text,
        identity_id -> Text,
        created_date -> Timestamp,
        allow_origins -> Nullable<Text>,
    }
}

diesel::table! {
    identity (identity_id) {
        identity_id -> Text,
        key_file_json -> Text,
        is_default -> Bool,
        is_deleted -> Bool,
        alias -> Nullable<Text>,
        note -> Nullable<Text>,
        created_date -> Timestamp,
    }
}

diesel::table! {
    identity_data (identity_id, module_id) {
        identity_id -> Nullable<Text>,
        module_id -> Nullable<Text>,
        configuration -> Nullable<Text>,
        version -> Nullable<Integer>,
    }
}

diesel::table! {
    role (id) {
        id -> Integer,
        name -> Text,
    }
}

diesel::table! {
    version_release (version) {
        version -> Nullable<Text>,
        name -> Text,
        seen -> Bool,
        release_ts -> Timestamp,
        insertion_ts -> Timestamp,
        update_ts -> Timestamp,
    }
}

diesel::joinable!(app_key -> identity (identity_id));
diesel::joinable!(app_key -> role (role_id));
diesel::joinable!(identity_data -> identity (identity_id));

diesel::allow_tables_to_appear_in_same_query!(
    app_key,
    identity,
    identity_data,
    role,
    version_release,
);
