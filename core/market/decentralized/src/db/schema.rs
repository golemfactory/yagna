table! {
    market_demand (id) {
        id -> Text,
        properties -> Text,
        constraints -> Text,
        node_id -> Text,
        creation_ts -> Timestamp,
        expiration_time -> Timestamp,
    }
}

table! {
    market_offer (id) {
        id -> Text,
        properties -> Text,
        constraints -> Text,
        node_id -> Text,
        creation_ts -> Timestamp,
        expiration_time -> Timestamp,
    }
}

allow_tables_to_appear_in_same_query!(market_demand, market_offer,);
