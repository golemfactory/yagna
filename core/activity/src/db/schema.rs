// Temporary database schema

table! {
    agreements (id) {
        id -> VarChar,
        requestor_id -> VarChar,
        provider_id -> VarChar,
    }
}

table! {
    activities (id) {
        id -> VarChar,
        agreement_id -> VarChar,
        state -> Nullable<VarChar>,
        usage -> Nullable<VarChar>,
    }
}

table! {
    events {
        id -> Integer,
        created_at -> Timestamp,
        data -> VarChar,
    }
}
