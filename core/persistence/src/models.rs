#![allow(unused)]
#![allow(clippy::all)]

use crate::schema::*;
use chrono::NaiveDateTime;
use diesel::sql_types::Integer;

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity"]
pub struct Activity {
    pub id: i32,
    pub natural_id: String,
    pub agreement_id: i32,
    pub state_id: i32,
    pub usage_id: i32,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity_event"]
pub struct ActivityEvent {
    pub id: i32,
    pub activity_id: i32,
    pub event_date: NaiveDateTime,
    pub event_type_id: i32,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity_event_type"]
pub struct ActivityEventType {
    pub id: i32,
    pub name: String,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity_state"]
pub struct ActivityState {
    pub id: i32,
    pub name: String,
    pub reason: Option<String>,
    pub error_message: Option<String>,
    pub updated_date: NaiveDateTime,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity_usage"]
pub struct ActivityUsage {
    pub id: i32,
    pub vector_json: Option<String>,
    pub updated_date: NaiveDateTime,
}

#[derive(Insertable)]
#[table_name = "agreement"]
pub struct NewAgreement {
    pub natural_id: String,
    pub state_id: AgreementState,
    pub demand_node_id: String,
    pub demand_properties_json: String,
    pub demand_constraints_json: String,
    pub offer_node_id: String,
    pub offer_properties_json: String,
    pub offer_constraints_json: String,
    pub proposed_signature: String,
    pub approved_signature: String,
    pub committed_signature: Option<String>,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "agreement"]
pub struct Agreement {
    pub id: i32,
    pub natural_id: String,
    pub state_id: i32,
    pub demand_natural_id: String,
    pub demand_node_id: String,
    pub demand_properties_json: String,
    pub demand_constraints_json: String,
    pub offer_natural_id: String,
    pub offer_node_id: String,
    pub offer_properties_json: String,
    pub offer_constraints_json: String,
    pub proposed_signature: String,
    pub approved_signature: String,
    pub committed_signature: String,
}

#[derive(AsExpression, FromSqlRow, PartialEq, Debug, Clone)]
#[sql_type = "Integer"]
pub enum AgreementState {
    New = 0,
    PendingApproval = 1,
    Approved = 10,
    Canceled = 40,
    Rejected = 41,
    Terminated = 50,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "agreement_event"]
pub struct AgreementEvent {
    pub id: i32,
    pub agreement_id: i32,
    pub event_date: NaiveDateTime,
    pub event_type_id: i32,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "agreement_event_type"]
pub struct AgreementEventType {
    pub id: i32,
    pub name: String,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "allocation"]
pub struct Allocation {
    pub id: i32,
    pub natural_id: String,
    pub created_date: NaiveDateTime,
    pub amount: String,
    pub remaining_amount: String,
    pub is_deposit: bool,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "debit_note"]
pub struct DebitNote {
    pub id: i32,
    pub natural_id: String,
    pub agreement_id: i32,
    pub state_id: i32,
    pub previous_note_id: Option<i32>,
    pub created_date: NaiveDateTime,
    pub activity_id: Option<i32>,
    pub total_amount_due: String,
    pub usage_counter_json: Option<String>,
    pub credit_account: String,
    pub payment_due_date: Option<NaiveDateTime>,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "invoice"]
pub struct Invoice {
    pub id: i32,
    pub natural_id: String,
    pub state_id: i32,
    pub last_debit_note_id: Option<i32>,
    pub created_date: NaiveDateTime,
    pub agreement_id: i32,
    pub amount: String,
    pub usage_counter_json: Option<String>,
    pub credit_account: String,
    pub payment_due_date: NaiveDateTime,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "invoice_debit_note_state"]
pub struct InvoiceDebitNoteState {
    pub id: i32,
    pub name: String,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "invoice_x_activity"]
pub struct InvoiceXActivity {
    pub id: i32,
    pub invoice_id: i32,
    pub activity_id: i32,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "payment"]
pub struct Payment {
    pub id: i32,
    pub natural_id: String,
    pub amount: String,
    pub debit_account: String,
    pub created_date: NaiveDateTime,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "payment_x_debit_note"]
pub struct PaymentXDebitNote {
    pub id: i32,
    pub payment_id: i32,
    pub debit_note_id: i32,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "payment_x_invoice"]
pub struct PaymentXInvoice {
    pub id: i32,
    pub payment_id: i32,
    pub invoice_id: i32,
}
