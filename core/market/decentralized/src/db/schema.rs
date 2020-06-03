table! {
    market_demand (id) {
        id -> Text,
        properties -> Text,
        constraints -> Text,
        node_id -> Text,
        creation_ts -> Timestamp,
        modification_ts -> Nullable<Timestamp>,
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
        modification_ts -> Nullable<Timestamp>,
        expiration_ts -> Timestamp,
    }
}

allow_tables_to_appear_in_same_query!(market_demand, market_offer,);
