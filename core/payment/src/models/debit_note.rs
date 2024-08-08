use crate::error::{DbError, DbResult};
use crate::schema::pay_debit_note;
use crate::utils::json_from_str;
use chrono::{NaiveDateTime, TimeZone, Utc};
use std::convert::{TryFrom, TryInto};
use uuid::Uuid;
use ya_client_model::payment::{DebitNote, DocumentStatus, NewDebitNote};
use ya_client_model::NodeId;
use ya_persistence::types::{BigDecimalField, Role};

#[derive(Insertable, Debug)]
#[table_name = "pay_debit_note"]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub previous_debit_note_id: Option<String>,
    pub activity_id: String,
    pub status: String,
    pub total_amount_due: BigDecimalField,
    pub usage_counter_vector: Option<Vec<u8>>,
    pub payment_due_date: Option<NaiveDateTime>,
    pub debit_nonce: i32
}

impl WriteObj {
    pub fn issued(
        debit_note: NewDebitNote,
        debit_nonce: i32,
        previous_debit_note_id: Option<String>,
        issuer_id: NodeId,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            owner_id: issuer_id,
            role: Role::Provider,
            previous_debit_note_id,
            activity_id: debit_note.activity_id,
            status: DocumentStatus::Issued.into(),
            total_amount_due: debit_note.total_amount_due.into(),
            usage_counter_vector: debit_note
                .usage_counter_vector
                .map(|v| v.to_string().into_bytes()),
            payment_due_date: debit_note.payment_due_date.map(|d| d.naive_utc()),
            debit_nonce
        }
    }

    pub fn received(debit_note: DebitNote, debit_nonce: i32, previous_debit_note_id: Option<String>) -> Self {
        Self {
            id: debit_note.debit_note_id,
            owner_id: debit_note.recipient_id,
            role: Role::Requestor,
            previous_debit_note_id,
            activity_id: debit_note.activity_id,
            status: DocumentStatus::Received.into(),
            total_amount_due: debit_note.total_amount_due.into(),
            usage_counter_vector: debit_note
                .usage_counter_vector
                .map(|v| v.to_string().into_bytes()),
            payment_due_date: debit_note.payment_due_date.map(|d| d.naive_utc()),
            debit_nonce
        }
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_debit_note"]
#[primary_key(id, owner_id)]
pub struct ReadObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub previous_debit_note_id: Option<String>,
    pub activity_id: String,
    pub status: String,
    pub timestamp: NaiveDateTime,
    pub total_amount_due: BigDecimalField,
    pub usage_counter_vector: Option<Vec<u8>>,
    pub payment_due_date: Option<NaiveDateTime>,
    pub debit_nonce: i32,

    pub agreement_id: String,     // From activity
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
}

impl TryFrom<ReadObj> for DebitNote {
    type Error = DbError;

    fn try_from(debit_note: ReadObj) -> DbResult<Self> {
        let issuer_id = debit_note.issuer_id();
        let recipient_id = debit_note.recipient_id();
        let usage_counter_vector = match debit_note.usage_counter_vector {
            Some(v) => Some(json_from_str(&String::from_utf8(v)?)?),
            None => None,
        };
        Ok(DebitNote {
            debit_note_id: debit_note.id,
            issuer_id,
            recipient_id,
            payee_addr: debit_note.payee_addr,
            payer_addr: debit_note.payer_addr,
            payment_platform: debit_note.payment_platform,
            previous_debit_note_id: debit_note.previous_debit_note_id,
            timestamp: Utc.from_utc_datetime(&debit_note.timestamp),
            agreement_id: debit_note.agreement_id,
            activity_id: debit_note.activity_id,
            total_amount_due: debit_note.total_amount_due.into(),
            usage_counter_vector,
            payment_due_date: debit_note
                .payment_due_date
                .map(|d| Utc.from_utc_datetime(&d)),
            status: debit_note.status.try_into()?,
            debit_nonce: debit_note.debit_nonce,
        })
    }
}
