use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::sql_types::Text;
use std::fmt;
use std::fmt::Debug;

use crate::db::model::{Agreement, AgreementId, AgreementState, Owner};
use crate::db::schema::market_agreement_event;

use std::str::FromStr;
use ya_client::model::market::agreement_event::AgreementTerminator;
use ya_client::model::market::{
    AgreementEventType as ClientEventType, AgreementOperationEvent as ClientEvent, Reason,
};
use ya_diesel_utils::DbTextField;
use ya_persistence::types::{AdaptTimestamp, TimestampAdapter};

#[derive(
    DbTextField,
    strum_macros::EnumString,
    derive_more::Display,
    AsExpression,
    FromSqlRow,
    PartialEq,
    Eq,
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

#[derive(DbTextField, Debug, Clone, AsExpression, FromSqlRow)]
#[sql_type = "Text"]
pub struct DbReason(pub Reason);

#[derive(Clone, Debug, Queryable)]
pub struct AgreementEvent {
    pub id: i32,
    pub agreement_id: AgreementId,
    pub event_type: AgreementEventType,
    pub timestamp: NaiveDateTime,
    pub issuer: Owner,
    pub reason: Option<DbReason>,
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Insertable)]
#[table_name = "market_agreement_event"]
pub struct NewAgreementEvent {
    pub agreement_id: AgreementId,
    pub event_type: AgreementEventType,
    pub timestamp: TimestampAdapter,
    pub issuer: Owner,
    pub reason: Option<DbReason>,
}

#[derive(thiserror::Error, Debug, Clone)]
#[error("Error creating Event from the Agreement: {0}")]
pub struct EventFromAgreementError(pub String);

impl NewAgreementEvent {
    pub(crate) fn new(
        agreement: &Agreement,
        reason: Option<Reason>,
        terminator: Owner,
        _timestamp: NaiveDateTime,
    ) -> Result<Self, EventFromAgreementError> {
        Ok(Self {
            agreement_id: agreement.id.clone(),
            event_type: match agreement.state {
                AgreementState::Pending
                | AgreementState::Proposal
                | AgreementState::Expired
                | AgreementState::Approving => {
                    let msg = format!("Wrong [{}] state {}", agreement.id, agreement.state);
                    log::error!("{}", msg);
                    return Err(EventFromAgreementError(msg));
                }
                AgreementState::Cancelled => AgreementEventType::Cancelled,
                AgreementState::Rejected => AgreementEventType::Rejected,
                AgreementState::Approved => AgreementEventType::Approved,
                AgreementState::Terminated => AgreementEventType::Terminated,
            },
            // We don't use timestamp from parameter here, because it came from other party
            // and we use this timestamp for sorting events, when returning them to caller.
            // On the other side, we should sign AgreementTerminated event together with timestamp,
            // so we need to have the same value on both nodes.
            // I don't know, how to solve this problem now, so I leave code that makes it possible to
            // add this external timestamp to database, but here I will use generated value.
            timestamp: Utc::now().adapt(),
            issuer: terminator,
            reason: reason.map(DbReason),
        })
    }
}

impl AgreementEvent {
    pub fn into_client(self) -> ClientEvent {
        let agreement_id = self.agreement_id.into_client();
        let event_date = DateTime::<Utc>::from_utc(self.timestamp, Utc);
        let reason = self.reason.map(|reason| reason.0);

        match self.event_type {
            AgreementEventType::Approved => ClientEvent {
                agreement_id,
                event_date,
                event_type: ClientEventType::AgreementApprovedEvent,
            },
            AgreementEventType::Cancelled => ClientEvent {
                agreement_id,
                event_date,
                event_type: ClientEventType::AgreementCancelledEvent { reason }
            },
            AgreementEventType::Rejected => ClientEvent {
                agreement_id,
                event_date,
                event_type: ClientEventType::AgreementRejectedEvent { reason }
            },
            AgreementEventType::Terminated => ClientEvent {
                agreement_id,
                event_date,
                event_type: ClientEventType::AgreementTerminatedEvent {
                    terminator: match self.issuer {
                        Owner::Provider => AgreementTerminator::Provider,
                        Owner::Requestor => AgreementTerminator::Requestor,
                    },
                    reason,
                    signature: self.signature.unwrap_or_else(|| {
                        // This shouldn't happen after https://github.com/golemfactory/yagna/issues/1079 is implemented.
                        log::trace!("AgreementTerminatedEvent without signature in database. Falling back to empty string.");
                        "".to_string()
                    }),
                }
            },
        }
    }
}

impl FromStr for DbReason {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(DbReason(serde_json::from_str::<Reason>(s)
            .map_err(|e| {
                log::warn!(
                    "Agreement Event with not parsable Reason in database. Error: {}. Shouldn't happen \
                     because market is responsible for rejecting invalid Reasons.", e
                )
            }
            ).ok().unwrap_or(Reason {
            message: "Invalid Reason in DB".into(),
            extra: Default::default()
        })))
    }
}

impl fmt::Display for DbReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_string(&self.0) {
            Ok(reason) => write!(f, "{}", reason),
            // It's impossible since Reason is serializable.
            Err(_) => write!(f, "Serialization failed!"),
        }
    }
}
