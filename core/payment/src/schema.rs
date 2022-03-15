table! {
    pay_activity (id, owner_id) {
        id -> Text,
        owner_id -> Text,
        role -> Text,
        agreement_id -> Text,
        total_amount_due -> Text,
        total_amount_accepted -> Text,
        total_amount_scheduled -> Text,
        total_amount_paid -> Text,
    }
}

table! {
    pay_activity_payment (payment_id, activity_id, owner_id) {
        payment_id -> Text,
        activity_id -> Text,
        owner_id -> Text,
        amount -> Text,
        allocation_id -> Nullable<Text>,
    }
}

table! {
    pay_agreement (id, owner_id) {
        id -> Text,
        owner_id -> Text,
        role -> Text,
        peer_id -> Text,
        payee_addr -> Text,
        payer_addr -> Text,
        payment_platform -> Text,
        total_amount_due -> Text,
        total_amount_accepted -> Text,
        total_amount_scheduled -> Text,
        total_amount_paid -> Text,
        app_session_id -> Nullable<Text>,
    }
}

table! {
    pay_agreement_payment (payment_id, agreement_id, owner_id) {
        payment_id -> Text,
        agreement_id -> Text,
        owner_id -> Text,
        amount -> Text,
        allocation_id -> Nullable<Text>,
    }
}

table! {
    pay_allocation (id) {
        id -> Text,
        owner_id -> Text,
        payment_platform -> Text,
        address -> Text,
        total_amount -> Text,
        spent_amount -> Text,
        remaining_amount -> Text,
        timestamp -> Timestamp,
        timeout -> Nullable<Timestamp>,
        make_deposit -> Bool,
        released -> Bool,
    }
}

table! {
    pay_debit_note (id, owner_id) {
        id -> Text,
        owner_id -> Text,
        role -> Text,
        previous_debit_note_id -> Nullable<Text>,
        activity_id -> Text,
        status -> Text,
        timestamp -> Timestamp,
        total_amount_due -> Text,
        usage_counter_vector -> Nullable<Binary>,
        payment_due_date -> Nullable<Timestamp>,
    }
}

table! {
    pay_debit_note_event (debit_note_id, event_type) {
        debit_note_id -> Text,
        owner_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        details -> Nullable<Text>,
    }
}

table! {
    pay_debit_note_event_read (debit_note_id, event_type) {
        debit_note_id -> Text,
        owner_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        details -> Nullable<Text>,
        app_session_id -> Nullable<Text>,
        id -> Text,
        role -> Text,
    }
}

table! {
    pay_document_status (status) {
        status -> Text,
    }
}

table! {
    pay_event_type (event_type) {
        event_type -> Text,
        role -> Text,
    }
}

table! {
    pay_invoice (id, owner_id) {
        id -> Text,
        owner_id -> Text,
        role -> Text,
        agreement_id -> Text,
        status -> Text,
        timestamp -> Timestamp,
        amount -> Text,
        payment_due_date -> Timestamp,
    }
}

table! {
    pay_invoice_event (invoice_id, event_type) {
        invoice_id -> Text,
        owner_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        details -> Nullable<Text>,
    }
}

table! {
    pay_invoice_event_read (invoice_id, event_type) {
        invoice_id -> Text,
        owner_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        details -> Nullable<Text>,
        app_session_id -> Nullable<Text>,
    }
}

table! {
    pay_invoice_x_activity (invoice_id, activity_id, owner_id) {
        invoice_id -> Text,
        activity_id -> Text,
        owner_id -> Text,
    }
}

table! {
    pay_order (id, driver) {
        id -> Text,
        driver -> Text,
        amount -> Text,
        payee_id -> Text,
        payer_id -> Text,
        payee_addr -> Text,
        payer_addr -> Text,
        payment_platform -> Text,
        invoice_id -> Nullable<Text>,
        debit_note_id -> Nullable<Text>,
        allocation_id -> Text,
        is_paid -> Bool,
    }
}

table! {
    pay_payment (id, owner_id) {
        id -> Text,
        owner_id -> Text,
        peer_id -> Text,
        payee_addr -> Text,
        payer_addr -> Text,
        payment_platform -> Text,
        role -> Text,
        amount -> Text,
        timestamp -> Timestamp,
        details -> Binary,
    }
}

joinable!(pay_activity_payment -> pay_allocation (allocation_id));
joinable!(pay_agreement_payment -> pay_allocation (allocation_id));
joinable!(pay_debit_note -> pay_document_status (status));
joinable!(pay_debit_note_event -> pay_event_type (event_type));
joinable!(pay_invoice -> pay_document_status (status));
joinable!(pay_invoice_event -> pay_event_type (event_type));
joinable!(pay_order -> pay_allocation (allocation_id));

allow_tables_to_appear_in_same_query!(
    pay_activity,
    pay_activity_payment,
    pay_agreement,
    pay_agreement_payment,
    pay_allocation,
    pay_debit_note,
    pay_debit_note_event,
    pay_debit_note_event_read,
    pay_document_status,
    pay_event_type,
    pay_invoice,
    pay_invoice_event,
    pay_invoice_event_read,
    pay_invoice_x_activity,
    pay_order,
    pay_payment,
);
