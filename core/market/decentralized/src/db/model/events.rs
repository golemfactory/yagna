use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::backend::Backend;
use diesel::deserialize;
use diesel::serialize::Output;
use diesel::sql_types::Integer;
use diesel::types::{FromSql, ToSql};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use thiserror::Error;

use ya_client::model::market::event::{ProviderEvent, RequestorEvent};
use ya_client::model::market::{Agreement as ClientAgreement, Proposal as ClientProposal};
use ya_client::model::ErrorMessage;
use ya_persistence::executor::DbExecutor;

use super::SubscriptionId;
use crate::db::dao::{AgreementDao, ProposalDao};
use crate::db::model::{Agreement, AgreementId, OwnerType, Proposal, ProposalId};
use crate::db::schema::market_event;
use crate::db::DbError;

#[derive(Error, Debug)]
pub enum EventError {
    #[error("Proposal [{0}] not found.")]
    ProposalNotFound(ProposalId),
    #[error("Proposal [{0}] not found.")]
    AgreementNotFound(AgreementId),
    #[error("Failed get proposal from database. Error: {0}.")]
    FailedGetProposal(DbError),
    #[error("Unexpected error: {0}.")]
    InternalError(#[from] ErrorMessage),
}

#[derive(FromPrimitive, AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[sql_type = "Integer"]
pub enum EventType {
    ProviderProposal = 1001,
    ProviderAgreement = 1002,
    ProviderPropertyQuery = 1003,
    RequestorProposal = 2001,
    RequestorPropertyQuery = 2002,
}

#[derive(Clone, Debug, Queryable)]
pub struct MarketEvent {
    pub id: i32,
    pub subscription_id: SubscriptionId,
    pub timestamp: NaiveDateTime,
    pub event_type: EventType,
    /// It can be Proposal, Agreement or structure,
    /// that will represent PropertyQuery.
    pub artifact_id: ProposalId,
}

#[derive(Clone, Debug, Insertable)]
#[table_name = "market_event"]
pub struct NewMarketEvent {
    pub subscription_id: SubscriptionId,
    pub event_type: EventType,
    pub artifact_id: ProposalId, // TODO: typed
}

impl MarketEvent {
    pub fn from_proposal(proposal: &Proposal, role: OwnerType) -> NewMarketEvent {
        NewMarketEvent {
            subscription_id: proposal.negotiation.subscription_id.clone(),
            event_type: match role {
                OwnerType::Requestor => EventType::RequestorProposal,
                OwnerType::Provider => EventType::ProviderProposal,
            },
            artifact_id: proposal.body.id.clone(),
        }
    }

    pub fn from_agreement(agreement: &Agreement) -> NewMarketEvent {
        NewMarketEvent {
            subscription_id: agreement.offer_id.clone(),
            event_type: EventType::ProviderAgreement,
            artifact_id: agreement.id.clone(),
        }
    }

    pub async fn into_client_requestor_event(
        self,
        db: &DbExecutor,
    ) -> Result<RequestorEvent, EventError> {
        match self.event_type {
            EventType::RequestorProposal => Ok(RequestorEvent::ProposalEvent {
                event_date: DateTime::<Utc>::from_utc(self.timestamp, Utc),
                proposal: self.into_client_proposal(db.clone()).await?,
            }),
            EventType::RequestorPropertyQuery => unimplemented!(),
            _ => Err(ErrorMessage::new(format!(
                "Wrong MarketEvent type [id={}]. Requestor event in Provider subscription.",
                self.id
            )))?,
        }
    }

    async fn into_client_proposal(self, db: DbExecutor) -> Result<ClientProposal, EventError> {
        let prop = db
            .as_dao::<ProposalDao>()
            .get_proposal(&self.artifact_id)
            .await
            .map_err(|error| EventError::FailedGetProposal(error))?
            .ok_or(EventError::ProposalNotFound(self.artifact_id.clone()))?;

        Ok(prop.into_client()?)
    }

    async fn into_client_agreement(self, db: DbExecutor) -> Result<ClientAgreement, EventError> {
        let prop = db
            .as_dao::<AgreementDao>()
            .select(&self.artifact_id, Utc::now().naive_utc())
            .await
            .map_err(|error| EventError::FailedGetProposal(error))?
            .ok_or(EventError::AgreementNotFound(self.artifact_id.clone()))?;

        Ok(prop.into_client()?)
    }

    pub async fn into_client_provider_event(
        self,
        db: &DbExecutor,
    ) -> Result<ProviderEvent, EventError> {
        match self.event_type {
            EventType::ProviderProposal => Ok(ProviderEvent::ProposalEvent {
                event_date: DateTime::<Utc>::from_utc(self.timestamp, Utc),
                proposal: self.into_client_proposal(db.clone()).await?,
            }),
            EventType::ProviderAgreement => Ok(ProviderEvent::AgreementEvent {
                event_date: DateTime::<Utc>::from_utc(self.timestamp, Utc),
                agreement: self.into_client_agreement(db.clone()).await?,
            }),
            EventType::ProviderPropertyQuery => unimplemented!(),
            _ => Err(ErrorMessage::new(format!(
                "Wrong MarketEvent type [id={}]. Requestor event in Provider subscription.",
                self.id
            )))?,
        }
    }
}

impl<DB: Backend> ToSql<Integer, DB> for EventType
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<W: std::io::Write>(&self, out: &mut Output<W, DB>) -> diesel::serialize::Result {
        (*self as i32).to_sql(out)
    }
}

impl<DB> FromSql<Integer, DB> for EventType
where
    i32: FromSql<Integer, DB>,
    DB: Backend,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        let enum_value = i32::from_sql(bytes)?;
        Ok(FromPrimitive::from_i32(enum_value).ok_or(anyhow::anyhow!(
            "Invalid conversion from {} (i32) to EventType.",
            enum_value
        ))?)
    }
}
