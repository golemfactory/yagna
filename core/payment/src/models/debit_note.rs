use crate::schema::pay_debit_note;
use chrono::{NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;
use ya_client_model::payment::{DebitNote, InvoiceStatus, NewDebitNote};
use ya_core_model::ethaddr::NodeId;
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
}

impl WriteObj {
    pub fn issued(
        debit_note: NewDebitNote,
        previous_debit_note_id: Option<String>,
        issuer_id: NodeId,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            owner_id: issuer_id,
            role: Role::Provider,
            previous_debit_note_id,
            activity_id: debit_note.activity_id,
            status: InvoiceStatus::Issued.into(),
            total_amount_due: debit_note.total_amount_due.into(),
            usage_counter_vector: debit_note
                .usage_counter_vector
                .map(|v| v.to_string().into_bytes()),
            payment_due_date: debit_note.payment_due_date.map(|d| d.naive_utc()),
        }
    }

    pub fn received(debit_note: DebitNote) -> Self {
        Self {
            id: debit_note.debit_note_id,
            owner_id: debit_note.recipient_id.parse().unwrap(),
            role: Role::Requestor,
            previous_debit_note_id: debit_note.previous_debit_note_id,
            activity_id: debit_note.activity_id,
            status: InvoiceStatus::Received.into(),
            total_amount_due: debit_note.total_amount_due.into(),
            usage_counter_vector: debit_note
                .usage_counter_vector
                .map(|v| v.to_string().into_bytes()),
            payment_due_date: debit_note.payment_due_date.map(|d| d.naive_utc()),
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

    pub agreement_id: String, // From activity
    pub peer_id: NodeId,      // From agreement
    pub payee_addr: String,   // From agreement
    pub payer_addr: String,   // From agreement
}

impl ReadObj {
    pub fn issuer_id(&self) -> String {
        match self.role {
            Role::Provider => self.owner_id.to_string(),
            Role::Requestor => self.peer_id.to_string(),
        }
    }

    pub fn recipient_id(&self) -> String {
        match self.role {
            Role::Provider => self.peer_id.to_string(),
            Role::Requestor => self.owner_id.to_string(),
        }
    }
}

impl From<ReadObj> for DebitNote {
    fn from(debit_note: ReadObj) -> Self {
        DebitNote {
            issuer_id: debit_note.issuer_id(),
            recipient_id: debit_note.recipient_id(),
            debit_note_id: debit_note.id,
            payee_addr: debit_note.payee_addr.to_string(),
            payer_addr: debit_note.payer_addr.to_string(),
            previous_debit_note_id: debit_note.previous_debit_note_id,
            timestamp: Utc.from_utc_datetime(&debit_note.timestamp),
            agreement_id: debit_note.agreement_id,
            activity_id: debit_note.activity_id,
            total_amount_due: debit_note.total_amount_due.into(),
            usage_counter_vector: debit_note
                .usage_counter_vector
                .map(|v| serde_json::from_str(&String::from_utf8(v).unwrap()).unwrap()),
            payment_due_date: debit_note
                .payment_due_date
                .map(|d| Utc.from_utc_datetime(&d)),
            status: debit_note.status.into(),
        }
    }
}
