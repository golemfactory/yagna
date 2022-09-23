use chrono::Utc;
use futures::stream::StreamExt;
use metrics::counter;
use std::sync::Arc;
use std::time::Instant;

use ya_client::model::market::{event::ProviderEvent, NewProposal, Reason};
use ya_core_model::NodeId;
use ya_service_api_web::middleware::Identity;
use ya_std_utils::LogErr;

use crate::db::{
    dao::{AgreementDao, NegotiationEventsDao, ProposalDao, SaveAgreementError},
    model::{Agreement, AgreementId, AgreementState, AppSessionId},
    model::{Issuer, Offer, Owner, Proposal, ProposalId, SubscriptionId},
    DbMixedExecutor,
};
use crate::matcher::store::SubscriptionStore;
use crate::protocol::negotiation::{error::*, messages::*, provider::NegotiationApi};

use super::common::CommonBroker;
use super::error::*;
use super::notifier::EventNotifier;
use crate::config::Config;
use crate::db::dao::AgreementDaoError;
use crate::negotiation::common::validate_transition;
use crate::negotiation::notifier::NotifierError;
use crate::utils::display::EnableDisplay;

#[derive(Clone, Debug, Eq, PartialEq, derive_more::Display)]
pub enum ApprovalResult {
    #[display(fmt = "Approved")]
    Approved,
    #[display(fmt = "Cancelled")]
    Cancelled,
}

/// Provider part of negotiation logic.
#[derive(Clone)]
pub struct ProviderBroker {
    pub(crate) common: CommonBroker,
    api: NegotiationApi,
}

impl ProviderBroker {
    pub fn new(
        db: DbMixedExecutor,
        store: SubscriptionStore,
        session_notifier: EventNotifier<AppSessionId>,
        config: Arc<Config>,
    ) -> Result<ProviderBroker, NegotiationInitError> {
        let broker = CommonBroker::new(db, store, session_notifier, config);

        let broker1 = broker.clone();
        let broker2 = broker.clone();
        let broker3 = broker.clone();
        let broker4 = broker.clone();
        let broker_proposal_reject = broker.clone();
        let broker_terminated = broker.clone();
        let commit_broker = broker.clone();

        let api = NegotiationApi::new(
            move |caller: String, msg: InitialProposalReceived| {
                on_initial_proposal(broker1.clone(), caller, msg)
            },
            move |caller: String, msg: ProposalReceived| {
                broker2
                    .clone()
                    .on_proposal_received(msg, caller, Owner::Requestor)
            },
            move |caller: String, msg: ProposalRejected| {
                broker_proposal_reject
                    .clone()
                    .on_proposal_rejected(msg, caller, Owner::Requestor)
            },
            move |caller: String, msg: AgreementReceived| {
                on_agreement_received(broker3.clone(), caller, msg)
            },
            move |caller: String, msg: AgreementCancelled| {
                on_agreement_cancelled(broker4.clone(), caller, msg)
            },
            move |caller: String, msg: AgreementTerminated| {
                broker_terminated
                    .clone()
                    .on_agreement_terminated(msg, caller, Owner::Requestor)
            },
            move |caller: String, msg: AgreementCommitted| {
                on_agreement_committed(commit_broker.clone(), caller, msg)
            },
        );

        // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
        // until first change to value will be made.
        counter!("market.agreements.provider.approved", 0);
        counter!("market.agreements.provider.proposed", 0);
        counter!("market.agreements.provider.terminated", 0);
        counter!("market.agreements.provider.terminated.reason", 0, "reason" => "NotSpecified");
        counter!("market.agreements.provider.terminated.reason", 0, "reason" => "Success");
        counter!("market.agreements.provider.approving", 0);
        counter!("market.agreements.provider.committing", 0);
        counter!("market.agreements.provider.rejected", 0);
        counter!("market.agreements.provider.cancelled", 0);
        counter!("market.events.provider.queried", 0);
        counter!("market.proposals.provider.countered", 0);
        counter!("market.proposals.provider.init-negotiation", 0);
        counter!("market.proposals.provider.received", 0);
        counter!("market.proposals.provider.rejected.initial", 0);
        counter!("market.proposals.provider.rejected.by-them", 0);
        counter!("market.proposals.provider.rejected.by-us", 0);

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

    pub async fn unsubscribe_offer(&self, id: &SubscriptionId) -> Result<(), NegotiationError> {
        self.common.unsubscribe(id).await
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
            .counter_proposal(
                offer_id,
                prev_proposal_id,
                proposal,
                &id.identity,
                Owner::Provider,
            )
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

    pub async fn reject_proposal(
        &self,
        offer_id: &SubscriptionId,
        proposal_id: &ProposalId,
        id: &Identity,
        reason: Option<Reason>,
    ) -> Result<(), RejectProposalError> {
        let proposal = self
            .common
            .reject_proposal(
                Some(offer_id),
                proposal_id,
                &id.identity,
                Owner::Provider,
                &reason,
            )
            .await?;

        self.api
            .reject_proposal(id.identity, &proposal, reason.clone())
            .await?;

        counter!("market.proposals.provider.rejected.by-us", 1);
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
            .query_events(offer_id, timeout, max_events, Owner::Provider)
            .await?;

        // Map model events to client RequestorEvent.
        let events = futures::stream::iter(events)
            .then(|event| event.into_client_provider_event(&self.common.db))
            .inspect(|result| {
                if let Err(error) = result {
                    log::error!("Error converting event to client type: {}", error);
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
    ) -> Result<ApprovalResult, AgreementError> {
        let stop_time = Instant::now() + std::time::Duration::from_secs_f64(timeout as f64);
        let dao = self.common.db.as_dao::<AgreementDao>();

        let agreement = {
            let _hold = self.common.agreement_lock.lock(&agreement_id).await;

            let agreement = dao
                .select(agreement_id, Some(id.identity), Utc::now().naive_utc())
                .await
                .map_err(|e| AgreementError::Get(agreement_id.to_string(), e))?
                .ok_or(AgreementError::NotFound(agreement_id.to_string()))?;

            if agreement.state == AgreementState::Cancelled {
                return Ok(ApprovalResult::Cancelled);
            }

            validate_transition(&agreement, AgreementState::Approving)?;

            // TODO: Sign Agreement.
            let signature = "NoSignature".to_string();
            let timestamp = Utc::now().naive_utc();

            let agreement = dao
                .approving(&agreement.id, &app_session_id, &signature, &timestamp)
                .await
                .map_err(|e| AgreementError::UpdateState(agreement.id.clone(), e))?;

            counter!("market.agreements.provider.approving", 1);
            if let Some(session) = app_session_id {
                log::info!(
                    "AppSession id [{}] set for Agreement [{}].",
                    &session,
                    &agreement.id
                );
            }
            agreement
        };

        // Listen to Agreements notification before we start sending message, because otherwise
        // we can lose events.
        let mut notifier = self.common.agreement_notifier.listen(&agreement.id);

        // It doesn't have to be under lock, since we have `Approving` state.
        // Note that this state change between `Approving` and `Pending` in both
        // ways is invisible for REST and GSB user, because `Approving` is only our
        // internal state and is mapped to `Pending`.
        //
        // Note: There's reason, that it CAN'T be done under lock. If we hold lock whole time
        // Requestor won't be able to cancel his Agreement proposal and he is allowed to do it.
        match self.api.approve_agreement(&agreement, timeout).await {
            Ok(_) => (),
            // It can turn out, that we are in `Cancelled` state since we weren't under
            // lock during `self.api.approve_agreement` execution. In such a case,
            // we shouldn't return error from here.
            Err(AgreementProtocolError::Remote(RemoteAgreementError::InvalidState(
                _,
                AgreementState::Cancelled,
            ))) => return Ok(ApprovalResult::Cancelled),
            Err(e) => {
                let _hold = self.common.agreement_lock.lock(&agreement_id).await;

                log::warn!(
                    "Failed to send Approve Agreement [{}] to Requestor. {}. Reverting state to `Pending`.",
                    agreement_id,
                    e,
                );
                dao.revert_approving(agreement_id).await.log_err().ok();
                Err(e)?
            }
        }

        counter!("market.agreements.provider.committing", 1);
        log::info!(
            "Provider {} approved Agreement [{}]. Waiting for commit from Requestor [{}].",
            id.display(),
            &agreement.id,
            agreement.requestor_id
        );

        // Here we must wait until `AgreementCommitted` message, since `approve_agreement`
        // is supposed to return after approval.
        match notifier.wait_for_event_until(stop_time).await {
            Err(NotifierError::Timeout(_)) => {
                let _hold = self.common.agreement_lock.lock(&agreement_id).await;
                dao.revert_approving(agreement_id).await.log_err().ok();

                Err(AgreementProtocolError::Timeout(agreement.id.clone()).into())
            }
            Err(error) => Err(AgreementError::Internal(format!(
                "Code logic error. Agreement events notifier shouldn't return: {}.",
                error
            ))),
            Ok(_) => Ok(()),
        }
        .log_err()?;

        {
            let _hold = self.common.agreement_lock.lock(&agreement_id).await;

            let agreement = dao
                .select(agreement_id, None, Utc::now().naive_utc())
                .await
                .map_err(|e| AgreementError::Get(agreement_id.to_string(), e))?
                .ok_or(AgreementError::Internal(format!(
                    "Agreement [{}], which existed previously, disappeared.",
                    agreement_id
                )))?;

            match agreement.state {
                AgreementState::Cancelled => Ok(ApprovalResult::Cancelled),
                AgreementState::Approved => Ok(ApprovalResult::Approved),
                AgreementState::Expired => Err(AgreementError::Expired(agreement.id))?,
                _ => Err(AgreementError::Internal(format!(
                    "Agreement [{}] has unexpected state [{}]",
                    agreement.id, agreement.state
                ))),
            }
        }
    }

    pub async fn reject_agreement(
        &self,
        id: &Identity,
        agreement_id: &AgreementId,
        reason: Option<Reason>,
    ) -> Result<(), AgreementError> {
        let dao = self.common.db.as_dao::<AgreementDao>();
        let agreement = {
            let _hold = self.common.agreement_lock.lock(&agreement_id).await;

            let agreement = dao
                .select(agreement_id, Some(id.identity), Utc::now().naive_utc())
                .await
                .map_err(|e| AgreementError::Get(agreement_id.to_string(), e))?
                .ok_or(AgreementError::NotFound(agreement_id.to_string()))?;

            validate_transition(&agreement, AgreementState::Rejected)?;

            let timestamp = Utc::now().naive_utc();
            self.api
                .reject_agreement(&agreement, reason.clone(), timestamp.clone())
                .await?;

            dao.reject(&agreement.id, reason.clone(), &timestamp)
                .await
                .map_err(|e| AgreementError::UpdateState((&agreement.id).clone(), e))?
        };

        counter!("market.agreements.provider.rejected", 1);
        log::info!(
            "Provider {} rejected Agreement [{}]. Reason: {}",
            id.display(),
            &agreement.id,
            reason.display(),
        );
        Ok(())
    }
}

async fn on_agreement_committed(
    broker: CommonBroker,
    caller: String,
    msg: AgreementCommitted,
) -> Result<(), CommitAgreementError> {
    let agreement_id = msg.agreement_id.clone();
    let caller_id = CommonBroker::parse_caller(&caller)?;
    agreement_committed(broker, caller_id, msg)
        .await
        .map_err(|e| CommitAgreementError::Remote(e, agreement_id))
}

async fn agreement_committed(
    broker: CommonBroker,
    caller: NodeId,
    msg: AgreementCommitted,
) -> Result<(), RemoteCommitAgreementError> {
    let dao = broker.db.as_dao::<AgreementDao>();
    let agreement = {
        let _hold = broker.agreement_lock.lock(&msg.agreement_id).await;

        // Note: we still validate caller here, because we can't be sure, that we were called
        // by the same Requestor.
        let agreement = dao
            .select(&msg.agreement_id, None, Utc::now().naive_utc())
            .await
            .map_err(|e| RemoteCommitAgreementError::Unexpected {
                public_msg: "Internal Error getting Agreement".to_string(),
                original_msg: e.to_string(),
            })
            .log_err()?
            .ok_or(RemoteCommitAgreementError::NotFound)?;

        if agreement.requestor_id != caller {
            // Don't reveal, that we know this Agreement id.
            Err(RemoteCommitAgreementError::NotFound)?
        }

        // Note: We can find out here, that our Agreement is already in `Cancelled` state, because
        // Requestor is allowed to call `cancel_agreement` at any time, before we commit Agreement.
        // In this case we should return here and `cancel_agreement` handler is responsible for
        // calling `notify_agreement`.
        match validate_transition(&agreement, AgreementState::Approved) {
            Ok(_) => Ok(()),
            Err(AgreementError::UpdateState(
                _,
                AgreementDaoError::InvalidTransition { from, .. },
            )) => match from {
                AgreementState::Cancelled => Err(RemoteCommitAgreementError::Cancelled),
                _ => Err(RemoteCommitAgreementError::InvalidState(from)),
            },
            Err(e) => Err(RemoteCommitAgreementError::Unexpected {
                public_msg: "Failed to validate Agreement state".to_string(),
                original_msg: e.to_string(),
            }),
        }
        .log_err()?;

        // TODO: Validate committed signature from message.

        dao.approve(&msg.agreement_id, &msg.signature)
            .await
            .map_err(|e| RemoteCommitAgreementError::Unexpected {
                public_msg: "Failed to update Agreement state to `Approved`.".to_string(),
                original_msg: e.to_string(),
            })
            .log_err()?
    };

    broker.notify_agreement(&agreement).await;

    counter!("market.agreements.provider.approved", 1);
    log::info!(
        "Agreement [{}] approved (committed) by [{}].",
        &agreement.id,
        &caller
    );
    Ok(())
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
    let offer = store.get_offer(&msg.offer_id).await?;

    // In this step we add Proposal, that was generated on Requestor by market.
    // This way we have the same state on Provider as on Requestor and we can use
    // the same function to handle this, as in normal counter_proposal flow.
    // TODO: Initial proposal id will differ on Requestor and Provider!! It isn't problem as long
    //  we don't log ids somewhere and try to compare between nodes.
    let caller_id = CommonBroker::parse_caller(&caller)?;

    let proposal = Proposal::new_provider(&msg.demand_id, caller_id, offer);
    let proposal_id = proposal.body.id.clone();
    let proposal = db
        .as_dao::<ProposalDao>()
        .save_initial_proposal(proposal)
        .await
        .map_err(|e| {
            ProposalValidationError::Internal(format!(
                "Failed saving initial Proposal [{}]: {}",
                proposal_id, e
            ))
        })?;

    // Now, since we have previous event in database, we can pretend that someone sent us
    // normal counter proposal.
    let msg = ProposalReceived {
        prev_proposal_id: proposal.body.id,
        proposal: msg.proposal,
    };
    broker
        .proposal_received(msg, caller_id, Owner::Requestor)
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

    if offer_proposal.body.issuer != Issuer::Us {
        return Err(RemoteProposeAgreementError::RequestorOwn(
            offer_proposal_id,
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
        Owner::Provider,
    );
    agreement.state = AgreementState::Pending;
    agreement.proposed_signature = Some(msg.signature);

    // TODO: Validate signature.

    // Check if we generated the same id, as Requestor sent us. If not, reject
    // it, because wrong generated ids could be not unique.
    let id = agreement.id.clone();
    if id != msg.agreement_id {
        Err(RemoteProposeAgreementError::InvalidId(id.clone()))?
    }

    // This is creation of Agreement, so lock is not needed yet.
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

async fn on_agreement_cancelled(
    broker: CommonBroker,
    caller: String,
    msg: AgreementCancelled,
) -> Result<(), AgreementProtocolError> {
    let caller: NodeId = CommonBroker::parse_caller(&caller)?;
    Ok(agreement_cancelled(broker, caller, msg)
        .await
        .map_err(|e| AgreementProtocolError::Remote(e))?)
}

async fn agreement_cancelled(
    broker: CommonBroker,
    caller: NodeId,
    msg: AgreementCancelled,
) -> Result<(), RemoteAgreementError> {
    let dao = broker.db.as_dao::<AgreementDao>();
    let agreement = {
        let _hold = broker.agreement_lock.lock(&msg.agreement_id).await;

        let agreement = dao
            .select(&msg.agreement_id, None, Utc::now().naive_utc())
            .await
            .log_err()
            .map_err(|_e| RemoteAgreementError::NotFound(msg.agreement_id.clone()))?
            .ok_or(RemoteAgreementError::NotFound(msg.agreement_id.clone()))?;

        if agreement.requestor_id != caller {
            // Don't reveal, that we know this Agreement id.
            Err(RemoteAgreementError::NotFound(msg.agreement_id.clone()))?
        }

        validate_transition(&agreement, AgreementState::Cancelled).map_err(|_| {
            RemoteAgreementError::InvalidState(agreement.id.clone(), agreement.state.clone())
        })?;

        dao.cancel(&agreement.id, msg.reason.clone(), &msg.cancellation_ts)
            .await
            .log_err()
            .map_err(|e| match e {
                AgreementDaoError::InvalidTransition { from, .. } => {
                    RemoteAgreementError::InvalidState(agreement.id.clone(), from)
                }
                _ => RemoteAgreementError::InternalError(agreement.id.clone()),
            })?
    };

    broker.notify_agreement(&agreement).await;

    counter!("market.agreements.provider.cancelled", 1);
    log::info!(
        "Agreement [{}] cancelled by [{}]. Reason: {}",
        &agreement.id,
        caller,
        msg.reason.display(),
    );
    Ok(())
}

impl From<GetProposalError> for RemoteProposeAgreementError {
    fn from(e: GetProposalError) -> Self {
        match e {
            GetProposalError::NotFound(id, ..) => RemoteProposeAgreementError::NotFound(id),
            GetProposalError::Internal(id, _, original_msg) => {
                RemoteProposeAgreementError::Unexpected {
                    public_msg: format!("Failed to get proposal from db [{}].", id),
                    original_msg,
                }
            }
        }
    }
}
