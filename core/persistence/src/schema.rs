table! {
    Activity (Id) {
        Id -> Integer,
        NaturalId -> Text,
        AgreementId -> Integer,
        StateId -> Integer,
    }
}

table! {
    ActivityEvent (Id) {
        Id -> Integer,
        ActivityId -> Integer,
        EventDate -> Timestamp,
        EventTypeId -> Integer,
    }
}

table! {
    ActivityEventType (Id) {
        Id -> Integer,
        Name -> Text,
    }
}

table! {
    ActivityState (Id) {
        Id -> Integer,
        Name -> Text,
    }
}

table! {
    Agreement (Id) {
        Id -> Integer,
        NaturalId -> Text,
        StateId -> Integer,
        DemandNaturalId -> Text,
        DemandNodeId -> Text,
        DemandPropertiesJson -> Text,
        DemandConstraintsJson -> Text,
        OfferNaturalId -> Text,
        OfferNodeId -> Text,
        OfferPropertiesJson -> Text,
        OfferConstraintsJson -> Text,
        ProposedSignature -> Text,
        ApprovedSignature -> Text,
        CommittedSignature -> Text,
    }
}

table! {
    AgreementEvent (Id) {
        Id -> Integer,
        AgreementId -> Integer,
        EventDate -> Timestamp,
        EventTypeId -> Integer,
    }
}

table! {
    AgreementEventType (Id) {
        Id -> Integer,
        Name -> Text,
    }
}

table! {
    AgreementState (Id) {
        Id -> Integer,
        Name -> Text,
    }
}

table! {
    Allocation (Id) {
        Id -> Integer,
        NaturalId -> Text,
        CreatedDate -> Timestamp,
        Amount -> Text,
        RemainingAmount -> Text,
        IsDeposit -> Text,
    }
}

table! {
    DebitNote (Id) {
        Id -> Integer,
        NaturalId -> Text,
        AgreementId -> Integer,
        StateId -> Integer,
        PreviousNoteId -> Nullable<Integer>,
        CreatedDate -> Timestamp,
        ActivityId -> Nullable<Integer>,
        TotalAmountDue -> Text,
        UsageCounterJson -> Nullable<Text>,
        CreditAccount -> Text,
        PaymentDueDate -> Nullable<Timestamp>,
    }
}

table! {
    Invoice (Id) {
        Id -> Integer,
        NaturalId -> Text,
        StateId -> Integer,
        LastDebitNoteId -> Nullable<Integer>,
        CreatedDate -> Timestamp,
        AgreementId -> Integer,
        Amount -> Text,
        UsageCounterJson -> Nullable<Text>,
        CreditAccount -> Text,
        PaymentDueDate -> Timestamp,
    }
}

table! {
    InvoiceDebitNoteState (Id) {
        Id -> Integer,
        Name -> Text,
    }
}

table! {
    InvoiceXActivity (Id) {
        Id -> Integer,
        InvoiceId -> Integer,
        ActivityId -> Integer,
    }
}

table! {
    Payment (Id) {
        Id -> Integer,
        NaturalId -> Text,
        Amount -> Text,
        DebitAccount -> Text,
        CreatedDate -> Timestamp,
    }
}

table! {
    PaymentXDebitNote (Id) {
        Id -> Integer,
        PaymentId -> Integer,
        DebitNoteId -> Integer,
    }
}

table! {
    PaymentXInvoice (Id) {
        Id -> Integer,
        PaymentId -> Integer,
        InvoiceId -> Integer,
    }
}

joinable!(Activity -> ActivityState (StateId));
joinable!(Activity -> Agreement (AgreementId));
joinable!(ActivityEvent -> Activity (ActivityId));
joinable!(ActivityEvent -> ActivityEventType (EventTypeId));
joinable!(Agreement -> AgreementState (StateId));
joinable!(AgreementEvent -> Agreement (AgreementId));
joinable!(AgreementEvent -> AgreementEventType (EventTypeId));
joinable!(DebitNote -> Activity (ActivityId));
joinable!(DebitNote -> Agreement (AgreementId));
joinable!(DebitNote -> InvoiceDebitNoteState (StateId));
joinable!(Invoice -> Agreement (AgreementId));
joinable!(Invoice -> InvoiceDebitNoteState (StateId));
joinable!(InvoiceXActivity -> Activity (ActivityId));
joinable!(InvoiceXActivity -> Invoice (InvoiceId));
joinable!(PaymentXDebitNote -> DebitNote (DebitNoteId));
joinable!(PaymentXDebitNote -> Payment (PaymentId));
joinable!(PaymentXInvoice -> Invoice (InvoiceId));
joinable!(PaymentXInvoice -> Payment (PaymentId));

allow_tables_to_appear_in_same_query!(
    Activity,
    ActivityEvent,
    ActivityEventType,
    ActivityState,
    Agreement,
    AgreementEvent,
    AgreementEventType,
    AgreementState,
    Allocation,
    DebitNote,
    Invoice,
    InvoiceDebitNoteState,
    InvoiceXActivity,
    Payment,
    PaymentXDebitNote,
    PaymentXInvoice,
);
