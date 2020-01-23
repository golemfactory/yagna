table! {
    allocation (id) {
        id -> Text,
        total_amount -> Integer,
        timeout -> Timestamp,
        make_deposit -> Bool,
    }
}

table! {
    debit_note (id) {
        id -> Text,
        previous_debit_note_id -> Nullable<Text>,
        agreement_id -> Text,
        activity_id -> Nullable<Text>,
        status -> Text,
        timestamp -> Timestamp,
        total_amount_due -> Integer,
        usage_counter_vector -> Nullable<Binary>,
        credit_account_id -> Text,
        payment_platform -> Nullable<Text>,
        payment_due_date -> Nullable<Timestamp>,
    }
}

table! {
    debit_note_event (debit_note_id, event_type) {
        debit_note_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        details -> Nullable<Text>,
    }
}

table! {
    invoice (id) {
        id -> Text,
        last_debit_note_id -> Text,
        agreement_id -> Text,
        status -> Text,
        timestamp -> Timestamp,
        amount -> Text,
        usage_counter_vector -> Nullable<Binary>,
        credit_account_id -> Text,
        payment_platform -> Nullable<Text>,
        payment_due_date -> Timestamp,
    }
}

table! {
    invoice_event (invoice_id, event_type) {
        invoice_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        details -> Nullable<Text>,
    }
}

table! {
    invoice_event_type (event_type) {
        event_type -> Text,
    }
}

table! {
    invoice_status (status) {
        status -> Text,
    }
}

table! {
    invoice_x_activity (invoice_id, activity_id) {
        invoice_id -> Text,
        activity_id -> Text,
    }
}

table! {
    payment (id) {
        id -> Text,
        amount -> Integer,
        timestamp -> Timestamp,
        allocation_id -> Nullable<Text>,
        details -> Text,
    }
}

table! {
    payment_x_debit_note (payment_id, debit_note_id) {
        payment_id -> Text,
        debit_note_id -> Text,
    }
}

table! {
    payment_x_invoice (payment_id, invoice_id) {
        payment_id -> Text,
        invoice_id -> Text,
    }
}

joinable!(debit_note -> invoice_status (status));
joinable!(debit_note_event -> debit_note (debit_note_id));
joinable!(debit_note_event -> invoice_event_type (event_type));
joinable!(invoice -> debit_note (last_debit_note_id));
joinable!(invoice -> invoice_status (status));
joinable!(invoice_event -> invoice (invoice_id));
joinable!(invoice_event -> invoice_event_type (event_type));
joinable!(invoice_x_activity -> invoice (invoice_id));
joinable!(payment -> allocation (allocation_id));
joinable!(payment_x_debit_note -> debit_note (debit_note_id));
joinable!(payment_x_debit_note -> payment (payment_id));
joinable!(payment_x_invoice -> invoice (invoice_id));
joinable!(payment_x_invoice -> payment (payment_id));

allow_tables_to_appear_in_same_query!(
    allocation,
    debit_note,
    debit_note_event,
    invoice,
    invoice_event,
    invoice_event_type,
    invoice_status,
    invoice_x_activity,
    payment,
    payment_x_debit_note,
    payment_x_invoice,
);
