table! {
    market_demand (id) {
        id -> Text,
        properties -> Text,
        constraints -> Text,
        node_id -> Text,
        creation_ts -> Timestamp,
        insertion_ts -> Nullable<Timestamp>,
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
        insertion_ts -> Nullable<Timestamp>,
        expiration_ts -> Timestamp,
    }
}

table! {
    market_offer_unsubscribed (id) {
        id -> Text,
        insertion_ts -> Timestamp,
        expiration_ts -> Timestamp,
        node_id -> Text,
    }
}

allow_tables_to_appear_in_same_query!(market_demand, market_offer, market_offer_unsubscribed);

joinable!(market_offer -> market_offer_unsubscribed (id));
