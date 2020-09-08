use crate::schema::pay_order;
use ya_client_model::NodeId;
use ya_core_model::payment::local::{PaymentTitle, SchedulePayment};
use ya_persistence::types::BigDecimalField;

#[derive(Debug, Insertable)]
#[table_name = "pay_order"]
pub struct WriteObj {
    pub id: String,
    pub driver: String,
    pub amount: BigDecimalField,
    pub payee_id: NodeId,
    pub payer_id: NodeId,
    pub payee_addr: String,
    pub payer_addr: String,
    pub payment_platform: String,
    pub invoice_id: Option<String>,
    pub debit_note_id: Option<String>,
    pub allocation_id: String,
    pub is_paid: bool,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_order"]
pub struct ReadObj {
    pub id: String,
    pub driver: String,
    pub amount: BigDecimalField,
    pub payee_id: NodeId,
    pub payer_id: NodeId,
    pub payee_addr: String,
    pub payer_addr: String,
    pub payment_platform: String,
    pub invoice_id: Option<String>,
    pub debit_note_id: Option<String>,
    pub allocation_id: String,
    pub is_paid: bool,

    pub agreement_id: Option<String>, // From invoice
    pub activity_id: Option<String>,  // From debit note
}

impl WriteObj {
    pub fn new(msg: SchedulePayment, id: String, driver: String) -> Self {
        let (invoice_id, debit_note_id) = match msg.title {
            PaymentTitle::DebitNote(title) => (None, Some(title.debit_note_id)),
            PaymentTitle::Invoice(title) => (Some(title.invoice_id), None),
        };
        Self {
            id,
            driver,
            amount: msg.amount.into(),
            payee_id: msg.payee_id,
            payer_id: msg.payer_id,
            payee_addr: msg.payee_addr,
            payer_addr: msg.payer_addr,
            payment_platform: msg.payment_platform,
            invoice_id,
            debit_note_id,
            allocation_id: msg.allocation_id,
            is_paid: false,
        }
    }
}
