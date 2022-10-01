use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::sql_types::Text;
use thiserror::Error;

use ya_client::model::market::event::{ProviderEvent, RequestorEvent};
use ya_client::model::market::{Agreement as ClientAgreement, Proposal as ClientProposal, Reason};
use ya_client::model::ErrorMessage;
use ya_diesel_utils::DbTextField;

use super::SubscriptionId;
use crate::db::dao::{AgreementDao, ProposalDao};
use crate::db::model::agreement_events::DbReason;
use crate::db::model::{Agreement, AgreementId, Owner, Proposal, ProposalId};
use crate::db::schema::market_negotiation_event;
use crate::db::DbMixedExecutor;

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
    strum_macros::Display,
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
    pub reason: Option<DbReason>,
}

#[derive(Clone, Debug, Insertable)]
#[table_name = "market_negotiation_event"]
pub struct NewMarketEvent {
    pub subscription_id: SubscriptionId,
    pub event_type: EventType,
    pub artifact_id: ProposalId, // TODO: typed
    pub reason: Option<DbReason>,
}

impl MarketEvent {
    pub fn from_proposal(proposal: &Proposal, role: Owner) -> NewMarketEvent {
        NewMarketEvent {
            subscription_id: proposal.negotiation.subscription_id.clone(),
            event_type: match role {
                Owner::Requestor => EventType::RequestorNewProposal,
                Owner::Provider => EventType::ProviderNewProposal,
            },
            artifact_id: proposal.body.id.clone(),
            reason: None,
        }
    }

    pub fn proposal_rejected(proposal: &Proposal, reason: Option<Reason>) -> NewMarketEvent {
        NewMarketEvent {
            subscription_id: proposal.negotiation.subscription_id.clone(),
            event_type: match proposal.body.id.owner() {
                Owner::Requestor => EventType::RequestorProposalRejected,
                Owner::Provider => EventType::ProviderProposalRejected,
            },
            artifact_id: proposal.body.id.clone(),
            reason: reason.map(|reason| DbReason(reason)),
        }
    }

    pub fn from_agreement(agreement: &Agreement) -> NewMarketEvent {
        NewMarketEvent {
            subscription_id: agreement.offer_id.clone(),
            event_type: EventType::ProviderAgreement,
            artifact_id: agreement.id.clone(),
            reason: None,
        }
    }

    pub async fn into_client_requestor_event(
        self,
        db: &DbMixedExecutor,
    ) -> Result<RequestorEvent, EventError> {
        let event_date = DateTime::<Utc>::from_utc(self.timestamp, Utc);
        match self.event_type {
            EventType::RequestorNewProposal => Ok(RequestorEvent::ProposalEvent {
                event_date,
                proposal: self.into_client_proposal(db.clone()).await?,
            }),
            EventType::RequestorProposalRejected => Ok(RequestorEvent::ProposalRejectedEvent {
                event_date,
                proposal_id: self.artifact_id.to_string(),
                reason: match self.reason {
                    None => None,
                    Some(reason) => Some(reason.0),
                },
            }),
            EventType::RequestorPropertyQuery => unimplemented!(),
            e => Err(ErrorMessage::new(format!(
                "Wrong MarketEvent type [{:?}]. Provider event on Requestor side not allowed.",
                e
            )))?,
        }
    }

    async fn into_client_proposal(self, db: DbMixedExecutor) -> Result<ClientProposal, EventError> {
        let prop = db
            .as_dao::<ProposalDao>()
            .get_proposal(&self.artifact_id)
            .await
            .map_err(|e| EventError::GetError(self.artifact_id.clone(), e.to_string()))?
            .ok_or_else(|| EventError::ProposalNotFound(self.artifact_id.clone()))?;

        Ok(prop.into_client()?)
    }

    async fn into_client_agreement(
        self,
        db: DbMixedExecutor,
    ) -> Result<ClientAgreement, EventError> {
        let agreement = db
            .as_dao::<AgreementDao>()
            .select(&self.artifact_id, None, Utc::now().naive_utc())
            .await
            .map_err(|e| EventError::GetError(self.artifact_id.clone(), e.to_string()))?
            .ok_or_else(|| EventError::AgreementNotFound(self.artifact_id.clone()))?;

        Ok(agreement.into_client()?)
    }

    pub async fn into_client_provider_event(
        self,
        db: &DbMixedExecutor,
    ) -> Result<ProviderEvent, EventError> {
        let event_date = DateTime::<Utc>::from_utc(self.timestamp, Utc);
        match self.event_type {
            EventType::ProviderNewProposal => Ok(ProviderEvent::ProposalEvent {
                event_date,
                proposal: self.into_client_proposal(db.clone()).await?,
            }),
            EventType::ProviderAgreement => Ok(ProviderEvent::AgreementEvent {
                event_date,
                agreement: self.into_client_agreement(db.clone()).await?,
            }),
            EventType::ProviderProposalRejected => Ok(ProviderEvent::ProposalRejectedEvent {
                event_date,
                proposal_id: self.artifact_id.to_string(),
                reason: match self.reason {
                    None => None,
                    Some(reason) => Some(reason.0),
                },
            }),
            EventType::ProviderPropertyQuery => unimplemented!(),
            e => Err(ErrorMessage::new(format!(
                "Wrong MarketEvent type [{:?}]. Requestor event in Provider side not allowed.",
                e
            )))?,
        }
    }
}
