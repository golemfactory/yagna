table! {
    payment (order_id) {
        order_id -> Text,
        amount -> Text,
        gas -> Text,
        sender -> Text,
        recipient -> Text,
        payment_due_date -> Timestamp,
        status -> Integer,
        tx_id -> Nullable<Text>,
        network -> Integer,
    }
}

table! {
    payment_status (status_id) {
        status_id -> Integer,
        status -> Text,
    }
}

table! {
    transaction (tx_id) {
        tx_id -> Text,
        sender -> Text,
        nonce -> Text,
        timestamp -> Timestamp,
        status -> Integer,
        tx_type -> Integer,
        encoded -> Text,
        signature -> Text,
        tx_hash -> Nullable<Text>,
        network -> Integer,
    }
}

table! {
    transaction_status (status_id) {
        status_id -> Integer,
        status -> Text,
    }
}

table! {
    transaction_type (type_id) {
        type_id -> Integer,
        tx_type -> Text,
    }
}

joinable!(payment -> payment_status (status));
joinable!(payment -> transaction (tx_id));
joinable!(transaction -> transaction_status (status));
joinable!(transaction -> transaction_type (tx_type));

allow_tables_to_appear_in_same_query!(
    payment,
    payment_status,
    transaction,
    transaction_status,
    transaction_type,
);
