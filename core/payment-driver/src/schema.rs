table! {
    gnt_driver_payment (invoice_id) {
        invoice_id -> Text,
        amount -> Text,
        gas -> Text,
        sender -> Text,
        recipient -> Text,
        payment_due_date -> Timestamp,
        status -> Integer,
        tx_id -> Nullable<Text>,
    }
}

table! {
    gnt_driver_payment_status (status_id) {
        status_id -> Integer,
        status -> Text,
    }
}

table! {
    gnt_driver_transaction (tx_id) {
        tx_id -> Text,
        sender -> Text,
        nonce -> Text,
        timestamp -> Timestamp,
        status -> Integer,
        encoded -> Text,
        signature -> Text,
        tx_hash -> Nullable<Text>,
    }
}

table! {
    gnt_driver_transaction_status (status_id) {
        status_id -> Integer,
        status -> Text,
    }
}

joinable!(gnt_driver_payment -> gnt_driver_payment_status (status));
joinable!(gnt_driver_payment -> gnt_driver_transaction (tx_id));
joinable!(gnt_driver_transaction -> gnt_driver_transaction_status (status));

allow_tables_to_appear_in_same_query!(
    gnt_driver_payment,
    gnt_driver_payment_status,
    gnt_driver_transaction,
    gnt_driver_transaction_status,
);
