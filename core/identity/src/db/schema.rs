table! {
    app_key (id) {
        id -> Integer,
        role_id -> Integer,
        name -> Text,
        key -> Text,
        identity_id -> Text,
        created_date -> Timestamp,
    }
}

table! {
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

table! {
    identity_data (identity_id, module_id) {
        identity_id -> Nullable<Text>,
        module_id -> Nullable<Text>,
        configuration -> Nullable<Text>,
        version -> Nullable<Integer>,
    }
}

table! {
    role (id) {
        id -> Integer,
        name -> Text,
    }
}

joinable!(app_key -> identity (identity_id));
joinable!(app_key -> role (role_id));
joinable!(identity_data -> identity (identity_id));

allow_tables_to_appear_in_same_query!(app_key, identity, identity_data, role,);
