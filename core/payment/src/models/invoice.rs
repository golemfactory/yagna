use crate::error::DbResult;
use crate::schema::{pay_invoice, pay_invoice_x_activity};
use chrono::{NaiveDateTime, TimeZone, Utc};
use std::convert::TryInto;
use uuid::Uuid;
use ya_client_model::payment::{DocumentStatus, Invoice, NewInvoice};
use ya_client_model::NodeId;
use ya_persistence::types::{BigDecimalField, Role};

#[derive(Debug, Insertable)]
#[table_name = "pay_invoice"]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub agreement_id: String,
    pub status: String,
    pub amount: BigDecimalField,
    pub payment_due_date: NaiveDateTime,
}

impl WriteObj {
    pub fn new_issued(invoice: NewInvoice, issuer_id: NodeId) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            owner_id: issuer_id,
            role: Role::Provider,
            agreement_id: invoice.agreement_id,
            status: DocumentStatus::Issued.into(),
            amount: invoice.amount.into(),
            payment_due_date: invoice.payment_due_date.naive_utc(),
        }
    }

    pub fn new_received(invoice: Invoice) -> Self {
        Self {
            id: invoice.invoice_id,
            owner_id: invoice.recipient_id,
            role: Role::Requestor,
            agreement_id: invoice.agreement_id,
            status: DocumentStatus::Received.into(),
            amount: invoice.amount.into(),
            payment_due_date: invoice.payment_due_date.naive_utc(),
        }
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_invoice"]
#[primary_key(id, owner_id)]
pub struct ReadObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub agreement_id: String,
    pub status: String,
    pub timestamp: NaiveDateTime,
    pub amount: BigDecimalField,
    pub payment_due_date: NaiveDateTime,

    pub peer_id: NodeId,          // From agreement
    pub payee_addr: String,       // From agreement
    pub payer_addr: String,       // From agreement
    pub payment_platform: String, // From agreement
}

impl ReadObj {
    pub fn issuer_id(&self) -> NodeId {
        match self.role {
            Role::Provider => self.owner_id,
            Role::Requestor => self.peer_id,
        }
    }

    pub fn recipient_id(&self) -> NodeId {
        match self.role {
            Role::Provider => self.peer_id,
            Role::Requestor => self.owner_id,
        }
    }

    pub fn into_api_model(self, activity_ids: Vec<String>) -> DbResult<Invoice> {
        Ok(Invoice {
            issuer_id: self.issuer_id(),
            recipient_id: self.recipient_id(),
            invoice_id: self.id,
            payee_addr: self.payee_addr,
            payer_addr: self.payer_addr,
            payment_platform: self.payment_platform,
            timestamp: Utc.from_utc_datetime(&self.timestamp),
            agreement_id: self.agreement_id,
            activity_ids,
            amount: self.amount.into(),
            payment_due_date: Utc.from_utc_datetime(&self.payment_due_date),
            status: self.status.try_into()?,
        })
    }
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_invoice_x_activity"]
#[primary_key(invoice_id, activity_id, owner_id)]
pub struct InvoiceXActivity {
    pub invoice_id: String,
    pub activity_id: String,
    pub owner_id: NodeId,
}

pub fn equivalent(read_invoice: &ReadObj, write_invoice: &WriteObj) -> bool {
    read_invoice.agreement_id == write_invoice.agreement_id
        && read_invoice.amount == write_invoice.amount
        && read_invoice.id == write_invoice.id
        && read_invoice.owner_id == write_invoice.owner_id
        && read_invoice.payment_due_date == write_invoice.payment_due_date
        && read_invoice.role == write_invoice.role
        && read_invoice.status == write_invoice.status
}
