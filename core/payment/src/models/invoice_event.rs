use crate::schema::pay_invoice_event;
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde::Serialize;
use std::convert::TryInto;
use ya_client_model::payment::{EventType, InvoiceEvent};
use ya_core_model::ethaddr::NodeId;

#[derive(Debug, Identifiable, Insertable)]
#[table_name = "pay_invoice_event"]
#[primary_key(invoice_id, event_type)]
pub struct WriteObj {
    pub invoice_id: String,
    pub owner_id: NodeId,
    pub event_type: String,
    pub details: Option<String>,
}

impl WriteObj {
    pub fn new<T: Serialize>(
        invoice_id: String,
        owner_id: NodeId,
        event_type: EventType,
        details: Option<T>,
    ) -> Self {
        Self {
            invoice_id,
            owner_id,
            event_type: event_type.into(),
            details: details.map(|s| serde_json::to_string(&s).unwrap()),
        }
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_invoice_event"]
#[primary_key(invoice_id, event_type)]
pub struct ReadObj {
    pub invoice_id: String,
    pub owner_id: NodeId,
    pub event_type: String,
    pub timestamp: NaiveDateTime,
    pub details: Option<String>,
}

impl From<ReadObj> for InvoiceEvent {
    fn from(event: ReadObj) -> Self {
        Self {
            invoice_id: event.invoice_id,
            timestamp: Utc.from_utc_datetime(&event.timestamp),
            details: event.details.map(|s| serde_json::from_str(&s).unwrap()),
            event_type: event.event_type.try_into().unwrap(),
        }
    }
}
