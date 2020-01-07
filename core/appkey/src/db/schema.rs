table! {
    app_key (id) {
        id -> Integer,
        role_id -> Integer,
        name -> Text,
        key -> Text,
        identity -> Text,
        created_date -> Timestamp,
    }
}

table! {
    role (id) {
        id -> Integer,
        name -> Text,
    }
}

joinable!(app_key -> role (role_id));

allow_tables_to_appear_in_same_query!(app_key, role,);
