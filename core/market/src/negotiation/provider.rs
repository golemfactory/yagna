use chrono::Utc;
use futures::stream::StreamExt;
use metrics::counter;
use std::str::FromStr;
use std::sync::Arc;

use ya_client::model::market::{event::ProviderEvent, NewProposal};
use ya_client::model::NodeId;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::{
    dao::{AgreementDao, NegotiationEventsDao, ProposalDao, SaveAgreementError},
    model::{Agreement, AgreementId, AgreementState, AppSessionId},
    model::{IssuerType, Offer, OwnerType, Proposal, ProposalId, SubscriptionId},
};
use crate::matcher::{error::QueryOfferError, store::SubscriptionStore};
use crate::protocol::negotiation::{error::*, messages::*, provider::NegotiationApi};

use super::common::CommonBroker;
use super::error::*;
use super::notifier::EventNotifier;
use crate::config::Config;
use crate::negotiation::common::expect_state;
use crate::utils::display::EnableDisplay;

/// Provider part of negotiation logic.
#[derive(Clone)]
pub struct ProviderBroker {
    pub(crate) common: CommonBroker,
    api: NegotiationApi,
}

impl ProviderBroker {
    pub fn new(
        db: DbExecutor,
        store: SubscriptionStore,
        session_notifier: EventNotifier<AppSessionId>,
        config: Arc<Config>,
    ) -> Result<ProviderBroker, NegotiationInitError> {
        let broker = CommonBroker::new(db.clone(), store, session_notifier, config);

        let broker1 = broker.clone();
        let broker2 = broker.clone();
        let broker3 = broker.clone();
        let broker_terminated = broker.clone();

        let api = NegotiationApi::new(
            move |caller: String, msg: InitialProposalReceived| {
                on_initial_proposal(broker1.clone(), caller, msg)
            },
            move |caller: String, msg: ProposalReceived| {
                broker2
                    .clone()
                    .on_proposal_received(caller, msg, OwnerType::Provider)
            },
            move |_caller: String, _msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementReceived| {
                on_agreement_received(broker3.clone(), caller, msg)
            },
            move |_caller: String, _msg: AgreementCancelled| async move { unimplemented!() },
            move |caller: String, msg: AgreementTerminated| {
                broker_terminated
                    .clone()
                    .on_agreement_terminated(caller, msg, OwnerType::Provider)
            },
        );

        // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
        // until first change to value will be made.
        counter!("market.events.provider.queried", 0);
        counter!("market.proposals.provider.received", 0);
        counter!("market.proposals.provider.countered", 0);
        counter!("market.proposals.provider.init-negotiation", 0);
        counter!("market.agreements.provider.proposed", 0);
        counter!("market.agreements.provider.approved", 0);

        Ok(ProviderBroker {
            api,
            common: broker,
        })
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        local_prefix: &str,
    ) -> Result<(), NegotiationInitError> {
        Ok(self.api.bind_gsb(public_prefix, local_prefix).await?)
    }

    pub async fn subscribe_offer(&self, _offer: &Offer) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_offer(
        &self,
        offer_id: &SubscriptionId,
    ) -> Result<(), NegotiationError> {
        self.common
            .negotiation_notifier
            .stop_notifying(offer_id)
            .await;
        Ok(())
    }

    pub async fn counter_proposal(
        &self,
        offer_id: &SubscriptionId,
        prev_proposal_id: &ProposalId,
        proposal: &NewProposal,
        id: &Identity,
    ) -> Result<ProposalId, ProposalError> {
        let (new_proposal, _) = self
            .common
            .counter_proposal(offer_id, prev_proposal_id, proposal, OwnerType::Provider)
            .await?;

        let proposal_id = new_proposal.body.id.clone();
        self.api
            .counter_proposal(new_proposal)
            .await
            .map_err(|e| ProposalError::Send(prev_proposal_id.clone(), e))?;

        counter!("market.proposals.provider.countered", 1);
        log::info!(
            "Provider {} countered Proposal [{}] with [{}]",
            id.display(),
            &prev_proposal_id,
            &proposal_id
        );
        Ok(proposal_id)
    }

    // TODO: this is only mock implementation not to throw 501
    pub async fn reject_proposal(
        &self,
        _offer_id: &SubscriptionId,
        proposal_id: &ProposalId,
        id: &Identity,
    ) -> Result<(), ProposalError> {
        // self.common
        //     .reject_proposal(offer_id, proposal_id, OwnerType::Provider)
        //     .await?;
        //
        // self.api
        //     .reject_proposal(id, proposal_id, TODO: requestor_id)
        //     .await
        //     .map_err(|e| ProposalError::Send(proposal_id.clone(), e))?;

        counter!("market.proposals.provider.rejected", 1);
        log::info!(
            "Provider {} rejected Proposal [{}]",
            id.display(),
            &proposal_id,
        );
        Ok(())
    }

    pub async fn query_events(
        &self,
        offer_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<ProviderEvent>, QueryEventsError> {
        let events = self
            .common
            .query_events(offer_id, timeout, max_events, OwnerType::Provider)
            .await?;

        // Map model events to client RequestorEvent.
        let events = futures::stream::iter(events)
            .then(|event| event.into_client_provider_event(&self.common.db))
            .inspect(|result| {
                if let Err(error) = result {
                    log::warn!("Error converting event to client type: {}", error);
                }
            })
            .filter_map(|event| async move { event.ok() })
            .collect::<Vec<ProviderEvent>>()
            .await;

        counter!("market.events.provider.queried", events.len() as u64);
        Ok(events)
    }

    pub async fn approve_agreement(
        &self,
        id: Identity,
        agreement_id: &AgreementId,
        app_session_id: AppSessionId,
        timeout: f32,
    ) -> Result<(), AgreementError> {
        let dao = self.common.db.as_dao::<AgreementDao>();
        let agreement = match dao
            .select(agreement_id, None, Utc::now().naive_utc())
            .await
            .map_err(|e| AgreementError::Get(agreement_id.clone(), e))?
        {
            None => Err(AgreementError::NotFound(agreement_id.clone()))?,
            Some(agreement) => agreement,
        };

        expect_state(&agreement, AgreementState::Pending)?;

        // TODO: possible race condition here ISSUE#430
        // 1. this state check should be also `db.update_state`
        // 2. `db.update_state` must be invoked after successful propose_agreement
        // TODO: if dao.approve fails, Provider and Requestor have inconsistent state.
        self.api.approve_agreement(&agreement, timeout).await?;
        dao.approve(agreement_id, &app_session_id)
            .await
            .map_err(|e| AgreementError::UpdateState(agreement_id.clone(), e))?;

        self.common.notify_agreement(&agreement).await;

        counter!("market.agreements.provider.approved", 1);
        log::info!(
            "Provider {} approved Agreement [{}].",
            id.display(),
            &agreement_id,
        );
        if let Some(session) = app_session_id {
            log::info!(
                "AppSession id [{}] set for Agreement [{}].",
                &session,
                &agreement_id
            );
        }
        return Ok(());
    }

    pub async fn terminate_agreement(
        &self,
        id: Identity,
        agreement_id: &AgreementId,
        reason: Option<String>,
    ) -> Result<(), AgreementError> {
        self.common
            .terminate_agreement(id, agreement_id, reason, OwnerType::Provider)
            .await
    }
}

// TODO: We need more elegant solution than this. This function still returns
//  CounterProposalError, which should be hidden in negotiation API and implementations
//  of handlers should return RemoteProposalError.
async fn on_initial_proposal(
    broker: CommonBroker,
    caller: String,
    msg: InitialProposalReceived,
) -> Result<(), CounterProposalError> {
    let proposal_id = msg.proposal.proposal_id.clone();
    initial_proposal(broker, caller, msg)
        .await
        .map_err(|e| CounterProposalError::Remote(e, proposal_id))
}

async fn initial_proposal(
    broker: CommonBroker,
    caller: String,
    msg: InitialProposalReceived,
) -> Result<(), RemoteProposalError> {
    let db = broker.db.clone();
    let store = broker.store.clone();

    // Check subscription.
    let offer = match store.get_offer(&msg.offer_id).await {
        Err(e) => match e {
            QueryOfferError::Unsubscribed(id) => Err(RemoteProposalError::Unsubscribed(id))?,
            QueryOfferError::Expired(id) => Err(RemoteProposalError::Expired(id))?,
            _ => Err(RemoteProposalError::Unexpected(e.to_string()))?,
        },
        Ok(offer) => offer,
    };

    // In this step we add Proposal, that was generated on Requestor by market.
    // This way we have the same state on Provider as on Requestor and we can use
    // the same function to handle this, as in normal counter_proposal flow.
    // TODO: Initial proposal id will differ on Requestor and Provider!! It isn't problem as long
    //  we don't log ids somewhere and try to compare between nodes.
    let owner_id =
        NodeId::from_str(&caller).map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;
    let proposal = Proposal::new_provider(&msg.demand_id, owner_id, offer);
    let proposal = db
        .as_dao::<ProposalDao>()
        .save_initial_proposal(proposal)
        .await
        .map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;

    // Now, since we have previous event in database, we can pretend that someone sent us
    // normal counter proposal.
    let received_msg = ProposalReceived {
        prev_proposal_id: proposal.body.id,
        proposal: msg.proposal,
    };
    broker
        .proposal_received(caller, received_msg, OwnerType::Provider)
        .await?;

    counter!("market.proposals.provider.init-negotiation", 1);
    Ok(())
}

async fn on_agreement_received(
    broker: CommonBroker,
    caller: String,
    msg: AgreementReceived,
) -> Result<(), ProposeAgreementError> {
    let id = msg.agreement_id.clone();
    agreement_received(broker, caller, msg)
        .await
        .map_err(|e| ProposeAgreementError::Remote(e.hide_sensitive_info(), id))
}

async fn agreement_received(
    broker: CommonBroker,
    caller: String,
    msg: AgreementReceived,
) -> Result<(), RemoteProposeAgreementError> {
    let offer_proposal = broker.get_proposal(None, &msg.proposal_id).await?;
    let offer_proposal_id = offer_proposal.body.id.clone();
    let offer_id = &offer_proposal.negotiation.offer_id.clone();

    if offer_proposal.body.issuer != IssuerType::Us {
        return Err(RemoteProposeAgreementError::RequestorOwn(
            offer_proposal_id.clone(),
        ));
    }

    let demand_proposal_id = offer_proposal
        .body
        .prev_proposal_id
        .clone()
        .ok_or_else(|| RemoteProposeAgreementError::NoNegotiations(offer_proposal_id))?;
    let demand_proposal = broker.get_proposal(None, &demand_proposal_id).await?;

    let mut agreement = Agreement::new_with_ts(
        demand_proposal,
        offer_proposal,
        msg.valid_to,
        msg.creation_ts,
        OwnerType::Provider,
    );
    agreement.state = AgreementState::Pending;

    // Check if we generated the same id, as Requestor sent us. If not, reject
    // it, because wrong generated ids could be not unique.
    let id = agreement.id.clone();
    if id != msg.agreement_id {
        Err(RemoteProposeAgreementError::InvalidId(id.clone()))?
    }

    let agreement = broker
        .db
        .as_dao::<AgreementDao>()
        .save(agreement)
        .await
        .map_err(|e| match e {
            SaveAgreementError::ProposalCountered(id) => {
                RemoteProposeAgreementError::AlreadyCountered(id)
            }
            _ => RemoteProposeAgreementError::Unexpected {
                public_msg: format!("Failed to save Agreement."),
                original_msg: e.to_string(),
            },
        })?;

    // TODO: If creating Agreement succeeds, but event can't be added, provider
    // TODO: will never approve Agreement. Solve problem when Event API will be available.
    broker
        .db
        .as_dao::<NegotiationEventsDao>()
        .add_agreement_event(&agreement)
        .await
        .map_err(|e| RemoteProposeAgreementError::Unexpected {
            public_msg: format!("Failed to add event for Agreement."),
            original_msg: e.to_string(),
        })?;

    // Send channel message to wake all query_events waiting for proposals.
    broker.negotiation_notifier.notify(&offer_id).await;

    counter!("market.agreements.provider.proposed", 1);
    log::info!(
        "Agreement proposal [{}] received from [{}].",
        &msg.agreement_id,
        &caller
    );
    Ok(())
}

impl From<GetProposalError> for RemoteProposeAgreementError {
    fn from(e: GetProposalError) -> Self {
        match e {
            GetProposalError::NotFound(id, ..) => RemoteProposeAgreementError::NotFound(id),
            GetProposalError::Internal(id, _, original_msg) => {
                RemoteProposeAgreementError::Unexpected {
                    public_msg: format!("Failed to get proposal from db [{}].", id.to_string()),
                    original_msg,
                }
            }
        }
    }
}
