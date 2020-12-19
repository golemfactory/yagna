use chrono::{DateTime, Utc};
use futures::stream::StreamExt;
use metrics::counter;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

use ya_client::model::market::{event::RequestorEvent, NewProposal};
use ya_client::model::{node_id::ParseError, NodeId};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::{
    dao::{AgreementDao, NegotiationEventsDao, ProposalDao, SaveAgreementError, StateError},
    model::{Agreement, AgreementId, AgreementState, AppSessionId},
    model::{Demand, IssuerType, OwnerType, Proposal, ProposalId, SubscriptionId},
    DbResult,
};
use crate::matcher::{store::SubscriptionStore, RawProposal};
use crate::protocol::negotiation::{error::*, messages::*, requestor::NegotiationApi};

use super::{common::*, error::*, notifier::NotifierError, EventNotifier};
use crate::config::Config;
use crate::db::dao::check_transition;
use crate::utils::display::EnableDisplay;

#[derive(Clone, derive_more::Display, Debug, PartialEq)]
pub enum ApprovalStatus {
    #[display(fmt = "Approved")]
    Approved,
    #[display(fmt = "Cancelled")]
    Cancelled,
    #[display(fmt = "Rejected")]
    Rejected,
}

/// Requestor part of negotiation logic.
pub struct RequestorBroker {
    pub(crate) common: CommonBroker,
    api: NegotiationApi,
}

impl RequestorBroker {
    pub fn new(
        db: DbExecutor,
        store: SubscriptionStore,
        proposal_receiver: UnboundedReceiver<RawProposal>,
        session_notifier: EventNotifier<AppSessionId>,
        config: Arc<Config>,
    ) -> Result<RequestorBroker, NegotiationInitError> {
        let broker = CommonBroker::new(db.clone(), store, session_notifier, config);

        let broker1 = broker.clone();
        let broker2 = broker.clone();
        let broker_terminated = broker.clone();
        let notifier = broker.negotiation_notifier.clone();

        let api = NegotiationApi::new(
            move |caller: String, msg: ProposalReceived| {
                broker1
                    .clone()
                    .on_proposal_received(caller, msg, OwnerType::Requestor)
            },
            move |_caller: String, _msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementApproved| {
                on_agreement_approved(broker2.clone(), caller, msg)
            },
            move |_caller: String, _msg: AgreementRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementTerminated| {
                broker_terminated
                    .clone()
                    .on_agreement_terminated(caller, msg, OwnerType::Requestor)
            },
        );

        let engine = RequestorBroker {
            api,
            common: broker,
        };

        // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
        // until first change to value will be made.
        counter!("market.agreements.requestor.created", 0);
        counter!("market.agreements.requestor.confirmed", 0);
        counter!("market.agreements.requestor.approved", 0);
        counter!("market.agreements.requestor.rejected", 0);
        counter!("market.agreements.requestor.cancelled", 0);
        counter!("market.proposals.requestor.generated", 0);
        counter!("market.proposals.requestor.received", 0);
        counter!("market.proposals.requestor.countered", 0);
        counter!("market.events.requestor.queried", 0);
        counter!("market.agreements.events.queried", 0);
        counter!("market.agreements.requestor.terminated", 0);
        counter!("market.agreements.requestor.terminated.reason", 0, "reason" => "NotSpecified");
        counter!("market.agreements.requestor.terminated.reason", 0, "reason" => "Success");

        tokio::spawn(proposal_receiver_thread(db, proposal_receiver, notifier));
        Ok(engine)
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        local_prefix: &str,
    ) -> Result<(), NegotiationInitError> {
        self.api.bind_gsb(public_prefix, local_prefix).await?;
        Ok(())
    }

    pub async fn subscribe_demand(&self, _demand: &Demand) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_demand(
        &self,
        demand_id: &SubscriptionId,
    ) -> Result<(), NegotiationError> {
        self.common
            .negotiation_notifier
            .stop_notifying(demand_id)
            .await;

        // We can ignore error, if removing events failed, because they will be never
        // queried again and don't collide with other subscriptions.
        let _ = self
            .common
            .db
            .as_dao::<NegotiationEventsDao>()
            .remove_events(demand_id)
            .await
            .map_err(|e| {
                log::warn!(
                    "Failed to remove events related to subscription [{}]. Error: {}.",
                    demand_id,
                    e
                )
            });
        // TODO: We could remove all resources related to Proposals.
        Ok(())
    }

    pub async fn counter_proposal(
        &self,
        demand_id: &SubscriptionId,
        prev_proposal_id: &ProposalId,
        proposal: &NewProposal,
        id: &Identity,
    ) -> Result<ProposalId, ProposalError> {
        let (new_proposal, is_first) = self
            .common
            .counter_proposal(demand_id, prev_proposal_id, proposal, OwnerType::Requestor)
            .await?;

        let proposal_id = new_proposal.body.id.clone();
        // Send Proposal to Provider. Note that it can be either our first communication with
        // Provider or we negotiated with him already, so we need to send different message in each
        // of these cases.
        match is_first {
            true => self.api.initial_proposal(new_proposal).await,
            false => self.api.counter_proposal(new_proposal).await,
        }
        .map_err(|e| ProposalError::Send(prev_proposal_id.clone(), e))?;

        counter!("market.proposals.requestor.countered", 1);
        log::info!(
            "Requestor {} countered Proposal [{}] with [{}]",
            id.display(),
            &prev_proposal_id,
            &proposal_id
        );
        Ok(proposal_id)
    }

    // TODO: this is only mock implementation not to throw 501
    pub async fn reject_proposal(
        &self,
        _demand_id: &SubscriptionId,
        proposal_id: &ProposalId,
        id: &Identity,
    ) -> Result<(), ProposalError> {
        // self.common
        //     .reject_proposal(demand_id, proposal_id, OwnerType::Requestor)
        //     .await?;
        //
        // self.api
        //     .reject_proposal(id, proposal_id, TODO: provider_id)
        //     .await
        //     .map_err(|e| ProposalError::Send(proposal_id.clone(), e))?;

        counter!("market.proposals.requestor.rejected", 1);
        log::info!(
            "Requestor {} rejected Proposal [{}]",
            id.display(),
            &proposal_id,
        );
        Ok(())
    }

    pub async fn query_events(
        &self,
        demand_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<RequestorEvent>, QueryEventsError> {
        let events = self
            .common
            .query_events(demand_id, timeout, max_events, OwnerType::Requestor)
            .await?;

        // Map model events to client RequestorEvent.
        let events = futures::stream::iter(events)
            .then(|event| event.into_client_requestor_event(&self.common.db))
            .inspect(|result| {
                if let Err(error) = result {
                    log::warn!("Error converting event to client type: {}", error);
                }
            })
            .filter_map(|event| async move { event.ok() })
            .collect::<Vec<RequestorEvent>>()
            .await;

        counter!("market.events.requestor.queried", events.len() as u64);
        Ok(events)
    }

    /// Initiates the Agreement handshake phase.
    ///
    /// Formulates an Agreement artifact from the Proposal indicated by the
    /// received Proposal Id.
    ///
    /// The Approval Expiry Date is added to Agreement artifact and implies
    /// the effective timeout on the whole Agreement Confirmation sequence.
    ///
    /// A successful call to `create_agreement` shall immediately be followed
    /// by a `confirm_agreement` and `wait_for_approval` call in order to listen
    /// for responses from the Provider.
    pub async fn create_agreement(
        &self,
        id: Identity,
        proposal_id: &ProposalId,
        valid_to: DateTime<Utc>,
    ) -> Result<AgreementId, AgreementError> {
        let offer_proposal_id = proposal_id;
        let offer_proposal = self
            .common
            .get_proposal(None, offer_proposal_id)
            .await
            .map_err(|e| AgreementError::from_proposal(proposal_id, e))?;

        // We can promote only Proposals, that we got from Provider.
        // Can't promote our own Proposal.
        if offer_proposal.body.issuer != IssuerType::Them {
            return Err(AgreementError::OwnProposal(proposal_id.clone()));
        }

        let demand_proposal_id = offer_proposal
            .body
            .prev_proposal_id
            .clone()
            .ok_or_else(|| AgreementError::NoNegotiations(offer_proposal_id.clone()))?;
        let demand_proposal = self
            .common
            .get_proposal(None, &demand_proposal_id)
            .await
            .map_err(|e| AgreementError::from_proposal(proposal_id, e))?;

        let agreement = Agreement::new(
            demand_proposal,
            offer_proposal,
            valid_to.naive_utc(),
            OwnerType::Requestor,
        );
        let agreement_id = agreement.id.clone();
        self.common
            .db
            .as_dao::<AgreementDao>()
            .save(agreement)
            .await
            .map_err(|e| match e {
                SaveAgreementError::Internal(e) => AgreementError::Save(proposal_id.clone(), e),
                SaveAgreementError::ProposalCountered(id) => AgreementError::ProposalCountered(id),
                SaveAgreementError::Exists(agreement_id, proposal_id) => {
                    AgreementError::AlreadyExists(agreement_id, proposal_id)
                }
            })?;

        counter!("market.agreements.requestor.created", 1);
        log::info!(
            "Requestor {} created Agreement [{}] from Proposal [{}].",
            id.display(),
            &agreement_id,
            &proposal_id
        );
        Ok(agreement_id)
    }

    pub async fn wait_for_approval(
        &self,
        id: &AgreementId,
        timeout: f32,
    ) -> Result<ApprovalStatus, WaitForApprovalError> {
        // TODO: Check if we are owner of Proposal
        // TODO: What to do with 2 simultaneous calls to wait_for_approval??
        //  should we reject one? And if so, how to discover, that two calls were made?
        let timeout = Duration::from_secs_f32(timeout.max(0.0));
        let mut notifier = self.common.agreement_notifier.listen(id);

        // Loop will wait for events notifications only one time. It doesn't have to be loop at all,
        // but it spares us doubled getting agreement and mapping statuses to return results.
        // So I think this simplification is worth confusion, that it cause.
        loop {
            let agreement = self
                .common
                .db
                .as_dao::<AgreementDao>()
                .select(id, None, Utc::now().naive_utc())
                .await
                .map_err(|e| WaitForApprovalError::Get(id.clone(), e))?
                .ok_or(WaitForApprovalError::NotFound(id.clone()))?;

            match agreement.state {
                AgreementState::Approved => {
                    counter!("market.agreements.requestor.approved", 1);
                    return Ok(ApprovalStatus::Approved);
                }
                AgreementState::Rejected => {
                    counter!("market.agreements.requestor.rejected", 1);
                    return Ok(ApprovalStatus::Rejected);
                }
                AgreementState::Cancelled => {
                    counter!("market.agreements.requestor.cancelled", 1);
                    return Ok(ApprovalStatus::Cancelled);
                }
                AgreementState::Expired => return Err(WaitForApprovalError::Expired(id.clone())),
                AgreementState::Proposal => {
                    return Err(WaitForApprovalError::NotConfirmed(id.clone()))
                }
                AgreementState::Terminated => {
                    return Err(WaitForApprovalError::Terminated(id.clone()))
                }
                AgreementState::Pending => (), // Still waiting for approval.
            };

            if let Err(error) = notifier.wait_for_event_with_timeout(timeout).await {
                return match error {
                    NotifierError::Timeout(_) => Err(WaitForApprovalError::Timeout(id.clone())),
                    NotifierError::ChannelClosed(_) => {
                        Err(WaitForApprovalError::Internal(error.to_string()))
                    }
                    NotifierError::Unsubscribed(_) => Ok(ApprovalStatus::Cancelled),
                };
            }
        }
    }

    /// Signs (not yet) Agreement self-created via `create_agreement`
    /// and sends it to the Provider.
    pub async fn confirm_agreement(
        &self,
        id: Identity,
        agreement_id: &AgreementId,
        app_session_id: AppSessionId,
    ) -> Result<(), AgreementError> {
        let dao = self.common.db.as_dao::<AgreementDao>();

        let agreement = match dao
            .select(
                agreement_id,
                Some(id.identity.clone()),
                Utc::now().naive_utc(),
            )
            .await
            .map_err(|e| AgreementError::Get(agreement_id.clone(), e))?
        {
            None => return Err(AgreementError::NotFound(agreement_id.clone())),
            Some(agreement) => agreement,
        };

        check_transition(agreement.state, AgreementState::Pending)
            .map_err(|e| AgreementError::UpdateState(agreement_id.clone(), e))?;

        // TODO : possible race condition here ISSUE#430
        // 1. this state check should be also `db.update_state`
        // 2. `db.update_state` must be invoked after successful propose_agreement
        self.api.propose_agreement(&agreement).await?;
        dao.confirm(agreement_id, &app_session_id)
            .await
            .map_err(|e| AgreementError::UpdateState(agreement_id.clone(), e))?;

        counter!("market.agreements.requestor.confirmed", 1);
        log::info!(
            "Requestor {} confirmed Agreement [{}] and sent to Provider.",
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
}

async fn on_agreement_approved(
    broker: CommonBroker,
    caller: String,
    msg: AgreementApproved,
) -> Result<(), ApproveAgreementError> {
    let caller: NodeId =
        caller
            .parse()
            .map_err(|e: ParseError| ApproveAgreementError::CallerParseError {
                e: e.to_string(),
                caller,
                id: msg.agreement_id.clone(),
            })?;
    Ok(agreement_approved(broker, caller, msg)
        .await
        .map_err(|e| ApproveAgreementError::Remote(e))?)
}

async fn agreement_approved(
    broker: CommonBroker,
    caller: NodeId,
    msg: AgreementApproved,
) -> Result<(), RemoteAgreementError> {
    let agreement = broker
        .db
        .as_dao::<AgreementDao>()
        .select(&msg.agreement_id, None, Utc::now().naive_utc())
        .await
        .map_err(|_e| RemoteAgreementError::NotFound(msg.agreement_id.clone()))?
        .ok_or(RemoteAgreementError::NotFound(msg.agreement_id.clone()))?;

    if agreement.provider_id != caller {
        // Don't reveal, that we know this Agreement id.
        Err(RemoteAgreementError::NotFound(msg.agreement_id.clone()))?
    }

    // TODO: Validate agreement signature.
    // Note: session must be None, because either we already set this value in ConfirmAgreement,
    // or we purposely left it None.
    broker
        .db
        .as_dao::<AgreementDao>()
        .approve(&msg.agreement_id, &None)
        .await
        .map_err(|err| match err {
            StateError::InvalidTransition { from, .. } => {
                match from {
                    // Expired Agreement could be InvalidState either, but we want to explicit
                    // say to provider, that Agreement has expired.
                    AgreementState::Expired => {
                        RemoteAgreementError::Expired(msg.agreement_id.clone())
                    }
                    _ => RemoteAgreementError::InvalidState(msg.agreement_id.clone(), from),
                }
            }
            e => {
                // Log our internal error, but don't reveal error message to Provider.
                log::warn!(
                    "Approve Agreement [{}] internal error: {}",
                    &msg.agreement_id,
                    e
                );
                RemoteAgreementError::InternalError(msg.agreement_id.clone())
            }
        })?;

    broker.notify_agreement(&agreement).await;
    log::info!(
        "Agreement [{}] approved by [{}].",
        &msg.agreement_id,
        &caller
    );
    Ok(())
}

pub async fn proposal_receiver_thread(
    db: DbExecutor,
    mut proposal_receiver: UnboundedReceiver<RawProposal>,
    notifier: EventNotifier<SubscriptionId>,
) {
    while let Some(proposal) = proposal_receiver.recv().await {
        let db = db.clone();
        let notifier = notifier.clone();
        match async move {
            log::debug!("Got matching Offer-Demand pair; emitting as Proposal to Requestor.");

            // Add proposal to database together with Negotiation record.
            let proposal = Proposal::new_requestor(proposal.demand, proposal.offer);
            let proposal = db
                .as_dao::<ProposalDao>()
                .save_initial_proposal(proposal)
                .await?;

            log::info!(
                "New Proposal [{}] (Offer [{}], Demand [{}])",
                proposal.body.id,
                proposal.negotiation.offer_id,
                proposal.negotiation.demand_id
            );

            // Create Proposal Event and add it to queue (database).
            let subscription_id = proposal.negotiation.subscription_id.clone();
            db.as_dao::<NegotiationEventsDao>()
                .add_proposal_event(proposal, OwnerType::Requestor)
                .await?;

            // Send channel message to wake all query_events waiting for proposals.
            counter!("market.proposals.requestor.generated", 1);
            notifier.notify(&subscription_id).await;
            DbResult::<()>::Ok(())
        }
        .await
        {
            Err(error) => log::warn!("Failed to add proposal. Error: {}", error),
            Ok(_) => (),
        }
    }
}
