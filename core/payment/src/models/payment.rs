use crate::schema::{pay_payment, pay_payment_x_debit_note, pay_payment_x_invoice};
use bigdecimal::BigDecimal;
use chrono::{NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;
use ya_client_model::payment::Payment;
use ya_client_model::NodeId;
use ya_persistence::types::{BigDecimalField, Role};

#[derive(Debug, Identifiable, Insertable)]
#[table_name = "pay_payment"]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: String,
    pub agreement_id: String,
    pub allocation_id: Option<String>,
    pub amount: BigDecimalField,
    pub details: Vec<u8>,
}

impl WriteObj {
    pub fn new_sent(
        payer_id: NodeId,
        agreement_id: String,
        allocation_id: String,
        amount: BigDecimal,
        details: Vec<u8>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            owner_id: payer_id,
            role: Role::Requestor.to_string(),
            agreement_id,
            allocation_id: Some(allocation_id),
            amount: amount.into(),
            details,
        }
    }

    pub fn new_received(payment: Payment, payee_id: NodeId) -> Self {
        Self {
            id: payment.payment_id,
            owner_id: payee_id,
            role: Role::Provider.to_string(),
            agreement_id: payment.agreement_id,
            allocation_id: None,
            amount: payment.amount.into(),
            details: base64::decode(&payment.details).unwrap(), // FIXME: unwrap
        }
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_payment"]
#[primary_key(id, owner_id)]
pub struct ReadObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub agreement_id: String,
    pub allocation_id: Option<String>,
    pub amount: BigDecimalField,
    pub timestamp: NaiveDateTime,
    pub details: Vec<u8>,

    pub peer_id: NodeId,    // From agreement
    pub payee_addr: String, // From agreement
    pub payer_addr: String, // From agreement
}

impl ReadObj {
    pub fn payee_id(&self) -> NodeId {
        match self.role {
            Role::Provider => self.owner_id,
            Role::Requestor => self.peer_id,
        }
    }

    pub fn payer_id(&self) -> NodeId {
        match self.role {
            Role::Provider => self.peer_id,
            Role::Requestor => self.owner_id,
        }
    }

    pub fn into_api_model(self, debit_note_ids: Vec<String>, invoice_ids: Vec<String>) -> Payment {
        Payment {
            payer_id: self.payer_id(),
            payee_id: self.payee_id(),
            payment_id: self.id,
            payer_addr: self.payer_addr,
            payee_addr: self.payee_addr,
            amount: self.amount.into(),
            timestamp: Utc.from_utc_datetime(&self.timestamp),
            agreement_id: self.agreement_id,
            allocation_id: self.allocation_id,
            debit_note_ids: Some(debit_note_ids),
            invoice_ids: Some(invoice_ids),
            details: base64::encode(&self.details),
        }
    }
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_payment_x_debit_note"]
#[primary_key(payment_id, debit_note_id, owner_id)]
pub struct PaymentXDebitNote {
    pub payment_id: String,
    pub debit_note_id: String,
    pub owner_id: NodeId,
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_payment_x_invoice"]
#[primary_key(payment_id, invoice_id, owner_id)]
pub struct PaymentXInvoice {
    pub payment_id: String,
    pub invoice_id: String,
    pub owner_id: NodeId,
}
