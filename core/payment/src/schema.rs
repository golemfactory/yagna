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
        created_ts -> Nullable<Timestamp>,
        updated_ts -> Nullable<Timestamp>,
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
        created_ts -> Nullable<Timestamp>,
        updated_ts -> Nullable<Timestamp>,
    }
}

table! {
    pay_allocation (owner_id, id) {
        id -> Text,
        owner_id -> Text,
        payment_platform -> Text,
        address -> Text,
        avail_amount -> Text,
        spent_amount -> Text,
        created_ts -> Timestamp,
        updated_ts -> Timestamp,
        timeout -> Timestamp,
        released -> Bool,
        deposit -> Nullable<Text>,
        deposit_status -> Nullable<Text>,
    }
}

table! {
    pay_allocation_expenditure (owner_id, allocation_id, agreement_id, activity_id) {
        owner_id -> Text,
        allocation_id -> Text,
        agreement_id -> Text,
        activity_id -> Nullable<Text>,
        accepted_amount -> Text,
        scheduled_amount -> Text,
    }
}

table! {
    pay_batch_cycle (owner_id, platform) {
        owner_id -> Text,
        platform -> Text,
        created_ts -> Timestamp,
        updated_ts -> Timestamp,
        cycle_interval -> Nullable<Text>,
        cycle_cron -> Nullable<Text>,
        cycle_last_process -> Nullable<Timestamp>,
        cycle_next_process -> Timestamp,
        cycle_max_interval -> Text,
        cycle_extra_pay_time -> Text,
    }
}

table! {
    pay_batch_order (owner_id, id) {
        id -> Text,
        created_ts -> Timestamp,
        updated_ts -> Timestamp,
        owner_id -> Text,
        payer_addr -> Text,
        platform -> Text,
        total_amount -> Text,
        paid_amount -> Text,
    }
}

table! {
    pay_batch_order_item (owner_id, order_id, payee_addr, allocation_id) {
        order_id -> Text,
        owner_id -> Text,
        payee_addr -> Text,
        allocation_id -> Text,
        amount -> Text,
        payment_id -> Nullable<Text>,
        paid -> Bool,
    }
}

table! {
    pay_batch_order_item_document (owner_id, order_id, payee_addr, allocation_id, agreement_id, activity_id) {
        order_id -> Text,
        owner_id -> Text,
        payee_addr -> Text,
        allocation_id -> Text,
        agreement_id -> Text,
        invoice_id -> Nullable<Text>,
        activity_id -> Nullable<Text>,
        debit_note_id -> Nullable<Text>,
        amount -> Text,
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
        debit_nonce -> Integer,
        send_accept -> Bool,
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
        role -> Text,
        debit_note_id -> Text,
        owner_id -> Text,
        event_type -> Text,
        timestamp -> Timestamp,
        details -> Nullable<Text>,
        app_session_id -> Nullable<Text>,
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
        send_accept -> Bool,
        send_reject -> Bool,
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
        role -> Text,
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
    pay_payment (id, owner_id, peer_id) {
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
        send_payment -> Bool,
        signature -> Nullable<Binary>,
        signed_bytes -> Nullable<Binary>,
    }
}

table! {
    pay_payment_document (payment_id, owner_id, peer_id, agreement_id, activity_id) {
        payment_id -> Text,
        owner_id -> Text,
        peer_id -> Text,
        agreement_id -> Text,
        invoice_id -> Nullable<Text>,
        activity_id -> Nullable<Text>,
        debit_note_id -> Nullable<Text>,
        amount -> Text,
    }
}

table! {
    pay_sync_needed_notifs (id) {
        id -> Text,
        last_ping -> Timestamp,
        retries -> Integer,
    }
}

joinable!(pay_debit_note -> pay_document_status (status));
joinable!(pay_debit_note_event -> pay_event_type (event_type));
joinable!(pay_invoice -> pay_document_status (status));
joinable!(pay_invoice_event -> pay_event_type (event_type));

allow_tables_to_appear_in_same_query!(
    pay_activity,
    pay_agreement,
    pay_allocation,
    pay_allocation_expenditure,
    pay_batch_cycle,
    pay_batch_order,
    pay_batch_order_item,
    pay_batch_order_item_document,
    pay_debit_note,
    pay_debit_note_event,
    pay_debit_note_event_read,
    pay_document_status,
    pay_event_type,
    pay_invoice,
    pay_invoice_event,
    pay_invoice_event_read,
    pay_invoice_x_activity,
    pay_payment,
    pay_payment_document,
);
