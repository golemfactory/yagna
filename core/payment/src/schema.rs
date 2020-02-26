table! {
    pay_allocation (id) {
        id -> Text,
        total_amount -> Text,
        timeout -> Nullable<Timestamp>,
        make_deposit -> Bool,
    }
}

table! {
    pay_debit_note (id) {
        id -> Text,
        issuer_id -> Text,
        recipient_id -> Text,
        previous_debit_note_id -> Nullable<Text>,
        agreement_id -> Text,
        activity_id -> Nullable<Text>,
        status -> Text,
        timestamp -> Timestamp,
        total_amount_due -> Text,
        usage_counter_vector -> Nullable<Binary>,
        credit_account_id -> Text,
        payment_platform -> Nullable<Text>,
        payment_due_date -> Nullable<Timestamp>,
    }
}

table! {
    pay_debit_note_event (debit_note_id, event_type) {
        debit_note_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        details -> Nullable<Text>,
    }
}

table! {
    pay_invoice (id) {
        id -> Text,
        issuer_id -> Text,
        recipient_id -> Text,
        last_debit_note_id -> Nullable<Text>,
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
    pay_invoice_event (invoice_id, event_type) {
        invoice_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        details -> Nullable<Text>,
    }
}

table! {
    pay_invoice_event_type (event_type) {
        event_type -> Text,
    }
}

table! {
    pay_invoice_status (status) {
        status -> Text,
    }
}

table! {
    pay_invoice_x_activity (invoice_id, activity_id) {
        invoice_id -> Text,
        activity_id -> Text,
    }
}

table! {
    pay_payment (id) {
        id -> Text,
        payer_id -> Text,
        payee_id -> Text,
        amount -> Text,
        timestamp -> Timestamp,
        allocation_id -> Nullable<Text>,
        details -> Text,
    }
}

table! {
    pay_payment_x_debit_note (payment_id, debit_note_id) {
        payment_id -> Text,
        debit_note_id -> Text,
    }
}

table! {
    pay_payment_x_invoice (payment_id, invoice_id) {
        payment_id -> Text,
        invoice_id -> Text,
    }
}

joinable!(pay_debit_note -> pay_invoice_status (status));
joinable!(pay_debit_note_event -> pay_debit_note (debit_note_id));
joinable!(pay_debit_note_event -> pay_invoice_event_type (event_type));
joinable!(pay_invoice -> pay_debit_note (last_debit_note_id));
joinable!(pay_invoice -> pay_invoice_status (status));
joinable!(pay_invoice_event -> pay_invoice (invoice_id));
joinable!(pay_invoice_event -> pay_invoice_event_type (event_type));
joinable!(pay_invoice_x_activity -> pay_invoice (invoice_id));
joinable!(pay_payment -> pay_allocation (allocation_id));
joinable!(pay_payment_x_debit_note -> pay_debit_note (debit_note_id));
joinable!(pay_payment_x_debit_note -> pay_payment (payment_id));
joinable!(pay_payment_x_invoice -> pay_invoice (invoice_id));
joinable!(pay_payment_x_invoice -> pay_payment (payment_id));

allow_tables_to_appear_in_same_query!(
    pay_allocation,
    pay_debit_note,
    pay_debit_note_event,
    pay_invoice,
    pay_invoice_event,
    pay_invoice_event_type,
    pay_invoice_status,
    pay_invoice_x_activity,
    pay_payment,
    pay_payment_x_debit_note,
    pay_payment_x_invoice,
);
