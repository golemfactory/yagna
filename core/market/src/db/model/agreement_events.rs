use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::backend::Backend;
use diesel::deserialize::{FromSql, Result as DeserializeResult};
use diesel::serialize::{Output, Result as SerializeResult, ToSql};
use diesel::sql_types::Text;
use std::io::Write;
use strum_macros::EnumString;

use crate::db::model::{AgreementId, OwnerType};
use crate::db::schema::market_agreement_event;
use crate::ya_client::model::market::event::AgreementEvent as ClientEvent;

#[derive(
    EnumString, derive_more::Display, AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy,
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
        let reason = self.reason;

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
                event_date,
                reason,
            },
        }
    }
}

impl<DB> ToSql<Text, DB> for AgreementEventType
where
    DB: Backend,
    String: ToSql<Text, DB>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> SerializeResult {
        self.to_string().to_sql(out)
    }
}

impl<DB> FromSql<Text, DB> for AgreementEventType
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> DeserializeResult<Self> {
        Ok(String::from_sql(bytes)?.parse()?)
    }
}
