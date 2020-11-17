use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::sql_types::Text;
use std::fmt::Debug;

use crate::db::model::{AgreementId, OwnerType};
use crate::db::schema::market_agreement_event;

use ya_client::model::market::agreement_event::AgreementTerminator;
use ya_client::model::market::{AgreementOperationEvent as ClientEvent, JsonReason};
use ya_diesel_utils::DatabaseTextField;

#[derive(
    DatabaseTextField,
    strum_macros::EnumString,
    derive_more::Display,
    AsExpression,
    FromSqlRow,
    PartialEq,
    Debug,
    Clone,
    Copy,
)]
#[sql_type = "Text"]
pub enum AgreementEventType {
    Approved,
    Rejected,
    Cancelled,
    Terminated,
}

#[derive(Clone, Debug, Queryable)]
pub struct AgreementEvent {
    pub id: i32,
    pub agreement_id: AgreementId,
    pub event_type: AgreementEventType,
    pub timestamp: NaiveDateTime,
    pub issuer: OwnerType,
    pub reason: Option<String>,
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Insertable)]
#[table_name = "market_agreement_event"]
pub struct NewAgreementEvent {
    pub agreement_id: AgreementId,
    pub event_type: AgreementEventType,
    pub issuer: OwnerType,
    pub reason: Option<String>,
}

impl AgreementEvent {
    pub fn into_client(self) -> ClientEvent {
        let agreement_id = self.agreement_id.into_client();
        let event_date = DateTime::<Utc>::from_utc(self.timestamp, Utc);
        let reason = self
            .reason
            .map(|reason| serde_json::from_str::<JsonReason>(&reason))
            .map(|result| result.map_err(|e| {
                log::warn!(
                    "Agreement Event with not parsable Reason in database. Error: {}. Shouldn't happen\
                     because market is responsible for rejecting invalid Reasons.", e
                )
            }).ok())
            .flatten();

        match self.event_type {
            AgreementEventType::Approved => ClientEvent::AgreementApprovedEvent {
                agreement_id,
                event_date,
            },
            AgreementEventType::Cancelled => ClientEvent::AgreementCancelledEvent {
                agreement_id,
                event_date,
                reason,
            },
            AgreementEventType::Rejected => ClientEvent::AgreementRejectedEvent {
                agreement_id,
                event_date,
                reason,
            },
            AgreementEventType::Terminated => ClientEvent::AgreementTerminatedEvent {
                agreement_id,
                terminator: match self.issuer {
                    OwnerType::Provider => AgreementTerminator::Provider,
                    OwnerType::Requestor => AgreementTerminator::Requestor,
                },
                event_date: event_date.to_rfc3339(), // TODO: this is bug in ya-client.
                reason,
                signature: self.signature.unwrap_or_else(|| {
                    log::warn!("AgreementTerminatedEvent without signature in database. This shouldn't happen, because\
                    Market is responsible for signing events and rejecting invalid signatures from other markets.");
                    "".to_string()
                }),
            },
        }
    }
}
