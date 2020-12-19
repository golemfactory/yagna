use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::sql_types::Text;
use thiserror::Error;

use ya_client::model::market::event::{ProviderEvent, RequestorEvent};
use ya_client::model::market::{Agreement as ClientAgreement, Proposal as ClientProposal};
use ya_client::model::ErrorMessage;
use ya_diesel_utils::DbTextField;
use ya_persistence::executor::DbExecutor;

use super::SubscriptionId;
use crate::db::dao::{AgreementDao, ProposalDao};
use crate::db::model::{Agreement, AgreementId, OwnerType, Proposal, ProposalId};
use crate::db::schema::market_negotiation_event;

#[derive(Error, Debug)]
pub enum EventError {
    #[error("Proposal [{0}] not found.")]
    ProposalNotFound(ProposalId),
    #[error("Proposal [{0}] not found.")]
    AgreementNotFound(AgreementId),
    #[error("Failed get Proposal or Agreement from database. Error: {0}.")]
    GetError(ProposalId, String),
    #[error("Unexpected error: {0}.")]
    InternalError(#[from] ErrorMessage),
}

#[derive(
    DbTextField,
    strum_macros::EnumString,
    strum_macros::ToString,
    AsExpression,
    FromSqlRow,
    PartialEq,
    Debug,
    Clone,
    Copy,
)]
#[sql_type = "Text"]
pub enum EventType {
    #[strum(serialize = "P-NewProposal")]
    ProviderNewProposal,
    #[strum(serialize = "P-ProposalRejected")]
    ProviderProposalRejected,
    #[strum(serialize = "P-Agreement")]
    ProviderAgreement,
    #[strum(serialize = "P-PropertyQuery")]
    ProviderPropertyQuery,
    #[strum(serialize = "R-NewProposal")]
    RequestorNewProposal,
    #[strum(serialize = "R-ProposalRejected")]
    RequestorProposalRejected,
    #[strum(serialize = "R-PropertyQuery")]
    RequestorPropertyQuery,
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
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Insertable)]
#[table_name = "market_negotiation_event"]
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
                OwnerType::Requestor => EventType::RequestorNewProposal,
                OwnerType::Provider => EventType::ProviderNewProposal,
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
            EventType::RequestorNewProposal => Ok(RequestorEvent::ProposalEvent {
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
            .map_err(|e| EventError::GetError(self.artifact_id.clone(), e.to_string()))?
            .ok_or(EventError::ProposalNotFound(self.artifact_id.clone()))?;

        Ok(prop.into_client()?)
    }

    async fn into_client_agreement(self, db: DbExecutor) -> Result<ClientAgreement, EventError> {
        let agreement = db
            .as_dao::<AgreementDao>()
            .select(&self.artifact_id, None, Utc::now().naive_utc())
            .await
            .map_err(|e| EventError::GetError(self.artifact_id.clone(), e.to_string()))?
            .ok_or(EventError::AgreementNotFound(self.artifact_id.clone()))?;

        Ok(agreement.into_client()?)
    }

    pub async fn into_client_provider_event(
        self,
        db: &DbExecutor,
    ) -> Result<ProviderEvent, EventError> {
        match self.event_type {
            EventType::ProviderNewProposal => Ok(ProviderEvent::ProposalEvent {
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
