use crate::error::{DbError, DbResult};
use crate::schema::pay_invoice_event;
use crate::utils::{json_from_str, json_to_string};
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde::Serialize;
use std::convert::{TryFrom, TryInto};
use ya_client_model::payment::{EventType, InvoiceEvent};
use ya_client_model::NodeId;

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
    ) -> Result<Self, DbError> {
        let details = match details {
            Some(details) => Some(json_to_string(&details)?),
            None => None,
        };
        Ok(Self {
            invoice_id,
            owner_id,
            event_type: event_type.into(),
            details,
        })
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

impl TryFrom<ReadObj> for InvoiceEvent {
    type Error = DbError;

    fn try_from(event: ReadObj) -> DbResult<Self> {
        // TODO Attach details when event_type=REJECTED
        // let details = match event.details {
        //     Some(s) => Some(json_from_str(&s)?),
        //     None => None,
        // };
        let event_type = event.event_type.try_into().unwrap();
        Ok(Self {
            invoice_id: event.invoice_id,
            event_date: Utc.from_utc_datetime(&event.timestamp),
            event_type
        })
    }
}
