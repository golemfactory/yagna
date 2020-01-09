table! {
    activity (id) {
        id -> Integer,
        natural_id -> Text,
        agreement_id -> Integer,
        state_id -> Integer,
        usage_id -> Integer,
    }
}

table! {
    activity_event (id) {
        id -> Integer,
        activity_id -> Integer,
        event_date -> Timestamp,
        event_type_id -> Integer,
    }
}

table! {
    activity_event_type (id) {
        id -> Integer,
        name -> Text,
    }
}

table! {
    activity_state (id) {
        id -> Integer,
        name -> Text,
        reason -> Nullable<Text>,
        error_message -> Nullable<Text>,
        updated_date -> Timestamp,
    }
}

table! {
    activity_usage (id) {
        id -> Integer,
        vector_json -> Nullable<Text>,
        updated_date -> Timestamp,
    }
}

table! {
    agreement (id) {
        id -> Integer,
        natural_id -> Text,
        state_id -> Integer,
        demand_natural_id -> Text,
        demand_node_id -> Text,
        demand_properties_json -> Text,
        demand_constraints_json -> Text,
        offer_natural_id -> Text,
        offer_node_id -> Text,
        offer_properties_json -> Text,
        offer_constraints_json -> Text,
        proposed_signature -> Text,
        approved_signature -> Text,
        committed_signature -> Text,
    }
}

table! {
    agreement_event (id) {
        id -> Integer,
        agreement_id -> Integer,
        event_date -> Timestamp,
        event_type_id -> Integer,
    }
}

table! {
    agreement_event_type (id) {
        id -> Integer,
        name -> Text,
    }
}

table! {
    agreement_state (id) {
        id -> Integer,
        name -> Text,
    }
}

table! {
    allocation (id) {
        id -> Integer,
        natural_id -> Text,
        created_date -> Timestamp,
        amount -> Text,
        remaining_amount -> Text,
        is_deposit -> Bool,
    }
}

table! {
    debit_note (id) {
        id -> Integer,
        natural_id -> Text,
        agreement_id -> Integer,
        state_id -> Integer,
        previous_note_id -> Nullable<Integer>,
        created_date -> Timestamp,
        activity_id -> Nullable<Integer>,
        total_amount_due -> Text,
        usage_counter_json -> Nullable<Text>,
        credit_account -> Text,
        payment_due_date -> Nullable<Timestamp>,
    }
}

table! {
    invoice (id) {
        id -> Integer,
        natural_id -> Text,
        state_id -> Integer,
        last_debit_note_id -> Nullable<Integer>,
        created_date -> Timestamp,
        agreement_id -> Integer,
        amount -> Text,
        usage_counter_json -> Nullable<Text>,
        credit_account -> Text,
        payment_due_date -> Timestamp,
    }
}

table! {
    invoice_debit_note_state (id) {
        id -> Integer,
        name -> Text,
    }
}

table! {
    invoice_x_activity (id) {
        id -> Integer,
        invoice_id -> Integer,
        activity_id -> Integer,
    }
}

table! {
    payment (id) {
        id -> Integer,
        natural_id -> Text,
        amount -> Text,
        debit_account -> Text,
        created_date -> Timestamp,
    }
}

table! {
    payment_x_debit_note (id) {
        id -> Integer,
        payment_id -> Integer,
        debit_note_id -> Integer,
    }
}

table! {
    payment_x_invoice (id) {
        id -> Integer,
        payment_id -> Integer,
        invoice_id -> Integer,
    }
}

joinable!(activity -> activity_state (state_id));
joinable!(activity -> activity_usage (usage_id));
joinable!(activity -> agreement (agreement_id));
joinable!(activity_event -> activity (activity_id));
joinable!(activity_event -> activity_event_type (event_type_id));
joinable!(agreement -> agreement_state (state_id));
joinable!(agreement_event -> agreement (agreement_id));
joinable!(agreement_event -> agreement_event_type (event_type_id));
joinable!(debit_note -> activity (activity_id));
joinable!(debit_note -> agreement (agreement_id));
joinable!(debit_note -> invoice_debit_note_state (state_id));
joinable!(invoice -> agreement (agreement_id));
joinable!(invoice -> invoice_debit_note_state (state_id));
joinable!(invoice_x_activity -> activity (activity_id));
joinable!(invoice_x_activity -> invoice (invoice_id));
joinable!(payment_x_debit_note -> debit_note (debit_note_id));
joinable!(payment_x_debit_note -> payment (payment_id));
joinable!(payment_x_invoice -> invoice (invoice_id));
joinable!(payment_x_invoice -> payment (payment_id));

allow_tables_to_appear_in_same_query!(
    activity,
    activity_event,
    activity_event_type,
    activity_state,
    activity_usage,
    agreement,
    agreement_event,
    agreement_event_type,
    agreement_state,
    allocation,
    debit_note,
    invoice,
    invoice_debit_note_state,
    invoice_x_activity,
    payment,
    payment_x_debit_note,
    payment_x_invoice,
);
