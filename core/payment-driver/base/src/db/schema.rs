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
        nonce -> Integer,
        status -> Integer,
        tx_type -> Integer,
        tmp_onchain_txs -> Nullable<Text>,
        final_tx -> Nullable<Text>,
        network -> Integer,
        starting_gas_price -> Nullable<Text>,
        current_gas_price -> Nullable<Text>,
        max_gas_price -> Nullable<Text>,
        final_gas_used -> Nullable<Integer>,
        amount_base -> Nullable<Text>,
        amount_erc20 -> Nullable<Text>,
        gas_limit -> Nullable<Integer>,
        time_created -> Timestamp,
        time_last_action -> Timestamp,
        time_sent -> Nullable<Timestamp>,
        time_confirmed -> Nullable<Timestamp>,
        last_error_msg -> Nullable<Text>,
        resent_times -> Integer,
        signature -> Nullable<Text>,
        encoded -> Text,
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
