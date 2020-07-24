use chrono::{DateTime, Utc};
use futures::stream::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

use ya_client::model::market::{event::RequestorEvent, proposal::Proposal as ClientProposal};
use ya_client::model::{node_id::ParseError, NodeId};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::{
    dao::{AgreementDao, EventsDao, ProposalDao, StateError},
    model::{Agreement, AgreementId, AgreementState, OwnerType},
    model::{Demand as ModelDemand, Proposal, ProposalId, SubscriptionId},
    DbResult,
};
use crate::matcher::{store::SubscriptionStore, RawProposal};
use crate::protocol::negotiation::{
    error::{ApproveAgreementError, RemoteAgreementError},
    messages::{AgreementApproved, AgreementRejected, ProposalReceived, ProposalRejected},
    requestor::NegotiationApi,
};

use super::{
    common::CommonBroker,
    error::{AgreementError, WaitForApprovalError},
    error::{NegotiationError, NegotiationInitError, ProposalError, QueryEventsError},
    notifier::NotifierError,
    EventNotifier,
};

#[derive(Clone, derive_more::Display, Debug)]
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
    common: CommonBroker,
    api: NegotiationApi,
    agreement_notifier: EventNotifier<AgreementId>,
}

impl RequestorBroker {
    pub fn new(
        db: DbExecutor,
        store: SubscriptionStore,
        proposal_receiver: UnboundedReceiver<RawProposal>,
    ) -> Result<RequestorBroker, NegotiationInitError> {
        let agreement_notifier = EventNotifier::new();
        let notifier = EventNotifier::new();
        let broker = CommonBroker {
            store,
            db: db.clone(),
            notifier: notifier.clone(),
        };

        let broker1 = broker.clone();
        let broker2 = broker.clone();
        let agreement_notifier2 = agreement_notifier.clone();
        let api = NegotiationApi::new(
            move |caller: String, msg: ProposalReceived| {
                broker1
                    .clone()
                    .on_proposal_received(caller, msg, OwnerType::Requestor)
            },
            move |_caller: String, _msg: ProposalRejected| async move { unimplemented!() },
            move |caller: String, msg: AgreementApproved| {
                on_agreement_approved(broker2.clone(), caller, msg, agreement_notifier2.clone())
            },
            move |_caller: String, _msg: AgreementRejected| async move { unimplemented!() },
        );

        let engine = RequestorBroker {
            api,
            common: broker,
            agreement_notifier,
        };

        tokio::spawn(proposal_receiver_thread(db, proposal_receiver, notifier));
        Ok(engine)
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), NegotiationInitError> {
        self.api.bind_gsb(public_prefix, private_prefix).await?;
        Ok(())
    }

    pub async fn subscribe_demand(&self, _demand: &ModelDemand) -> Result<(), NegotiationError> {
        // TODO: Implement
        Ok(())
    }

    pub async fn unsubscribe_demand(
        &self,
        demand_id: &SubscriptionId,
    ) -> Result<(), NegotiationError> {
        self.common.notifier.stop_notifying(demand_id).await;

        // We can ignore error, if removing events failed, because they will be never
        // queried again and don't collide with other subscriptions.
        let _ = self
            .common
            .db
            .as_dao::<EventsDao>()
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
        proposal: &ClientProposal,
    ) -> Result<ProposalId, ProposalError> {
        let (new_proposal, is_initial) = self
            .common
            .counter_proposal(demand_id, prev_proposal_id, proposal, OwnerType::Requestor)
            .await?;

        let proposal_id = new_proposal.body.id.clone();
        // Send Proposal to Provider. Note that it can be either our first communication with
        // Provider or we negotiated with him already, so we need to send different message in each
        // of these cases.
        match is_initial {
            true => self.api.initial_proposal(new_proposal).await,
            false => self.api.counter_proposal(new_proposal).await,
        }
        .map_err(|e| ProposalError::FailedSendProposal(prev_proposal_id.clone(), e))?;

        Ok(proposal_id)
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
        Ok(futures::stream::iter(events)
            .then(|event| event.into_client_requestor_event(&self.common.db))
            .inspect(|result| {
                if let Err(error) = result {
                    log::warn!("Error converting event to client type: {}", error);
                }
            })
            .filter_map(|event| async move { event.ok() })
            .collect::<Vec<RequestorEvent>>()
            .await)
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
    ///
    /// TODO: **Note**: Moves given Proposal to `Approved` state.
    pub async fn create_agreement(
        &self,
        _id: Identity,
        proposal_id: &ProposalId,
        valid_to: DateTime<Utc>,
    ) -> Result<AgreementId, AgreementError> {
        // TODO: Check if we are owner of Proposal
        let offer_proposal_id = proposal_id;
        let offer_proposal = self
            .common
            .get_proposal(offer_proposal_id)
            .await
            .map_err(|e| AgreementError::from(proposal_id, e))?;

        let demand_proposal_id = offer_proposal
            .body
            .prev_proposal_id
            .clone()
            .ok_or_else(|| AgreementError::NoNegotiations(offer_proposal_id.clone()))?;
        let demand_proposal = self
            .common
            .get_proposal(&demand_proposal_id)
            .await
            .map_err(|e| AgreementError::from(proposal_id, e))?;

        let agreement = Agreement::new(
            demand_proposal,
            offer_proposal,
            valid_to.naive_utc(),
            OwnerType::Requestor,
        );
        let id = agreement.id.clone();
        self.common
            .db
            .as_dao::<AgreementDao>()
            .save(agreement)
            .await
            .map_err(|e| AgreementError::Save(proposal_id.clone(), e))?;
        Ok(id)
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
        let mut notifier = self.agreement_notifier.listen(id);

        // Loop will wait for events notifications only one time. It doesn't have to be loop at all,
        // but it spares us doubled getting agreement and mapping statuses to return results.
        // So I think this simplification is worth confusion, that it cause.
        loop {
            let agreement = self
                .common
                .db
                .as_dao::<AgreementDao>()
                .select(id, Utc::now().naive_utc())
                .await
                .map_err(|e| WaitForApprovalError::FailedGetFromDb(id.clone(), e))?
                .ok_or(WaitForApprovalError::NotFound(id.clone()))?;

            match agreement.state {
                AgreementState::Approved => return Ok(ApprovalStatus::Approved),
                AgreementState::Rejected => return Ok(ApprovalStatus::Rejected),
                AgreementState::Cancelled => return Ok(ApprovalStatus::Cancelled),
                AgreementState::Expired => {
                    return Err(WaitForApprovalError::AgreementExpired(id.clone()))
                }
                AgreementState::Proposal => {
                    return Err(WaitForApprovalError::AgreementNotConfirmed(id.clone()))
                }
                AgreementState::Terminated => {
                    return Err(WaitForApprovalError::AgreementTerminated(id.clone()))
                }
                AgreementState::Pending => (), // Still waiting for approval.
            };

            if let Err(error) = notifier.wait_for_event_with_timeout(timeout).await {
                return match error {
                    NotifierError::Timeout(_) => Err(WaitForApprovalError::Timeout(id.clone())),
                    NotifierError::ChannelClosed(_) => {
                        Err(WaitForApprovalError::InternalError(error.to_string()))
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
        _id: Identity,
        agreement_id: &AgreementId,
    ) -> Result<(), AgreementError> {
        let dao = self.common.db.as_dao::<AgreementDao>();

        let mut agreement = match dao
            .select(agreement_id, Utc::now().naive_utc())
            .await
            .map_err(|e| AgreementError::Get(agreement_id.clone(), e))?
        {
            None => return Err(AgreementError::NotFound(agreement_id.clone())),
            Some(agreement) => agreement,
        };

        Err(match agreement.state {
            AgreementState::Proposal => {
                // TODO : possible race condition here ISSUE#430
                // 1. this state check should be also `db.update_state`
                // 2. `db.update_state` must be invoked after successful propose_agreement
                agreement.state = AgreementState::Pending;
                self.api.propose_agreement(agreement).await?;
                dao.update_state(agreement_id, AgreementState::Pending)
                    .await
                    .map_err(|e| AgreementError::Get(agreement_id.clone(), e))?;
                return Ok(());
            }
            AgreementState::Pending => AgreementError::Confirmed(agreement.id),
            AgreementState::Cancelled => AgreementError::Cancelled(agreement.id),
            AgreementState::Rejected => AgreementError::Rejected(agreement.id),
            AgreementState::Approved => AgreementError::Approved(agreement.id),
            AgreementState::Expired => AgreementError::Expired(agreement.id),
            AgreementState::Terminated => AgreementError::Terminated(agreement.id),
        })
    }
}

async fn on_agreement_approved(
    broker: CommonBroker,
    caller: String,
    msg: AgreementApproved,
    notifier: EventNotifier<AgreementId>,
) -> Result<(), ApproveAgreementError> {
    let agreement_id = msg.agreement_id.clone();
    let caller: NodeId =
        caller
            .parse()
            .map_err(|e: ParseError| ApproveAgreementError::CallerParseError {
                e: e.to_string(),
                caller,
                id: agreement_id.clone(),
            })?;
    agreement_approved(broker, caller, msg, notifier)
        .await
        .map_err(|e| ApproveAgreementError::Remote(e, agreement_id))
}

async fn agreement_approved(
    broker: CommonBroker,
    caller: NodeId,
    msg: AgreementApproved,
    notifier: EventNotifier<AgreementId>,
) -> Result<(), RemoteAgreementError> {
    let agreement = broker
        .db
        .as_dao::<AgreementDao>()
        .select(&msg.agreement_id, Utc::now().naive_utc())
        .await
        .map_err(|_e| RemoteAgreementError::NotFound(msg.agreement_id.clone()))?
        .ok_or(RemoteAgreementError::NotFound(msg.agreement_id.clone()))?;

    if agreement.provider_id != caller {
        // Don't reveal, that we know this Agreement id.
        Err(RemoteAgreementError::NotFound(msg.agreement_id.clone()))?
    }

    // TODO: Validate agreement signature.
    broker
        .db
        .as_dao::<AgreementDao>()
        .approve(&msg.agreement_id)
        .await
        .map_err(|e| match e {
            StateError::InvalidTransition { id, from, .. } => {
                match from {
                    // Expired Agreement could be InvalidState either, but we want to explicit
                    // say to provider, that Agreement has expired.
                    AgreementState::Expired => RemoteAgreementError::Expired(id),
                    _ => RemoteAgreementError::InvalidState(id, from),
                }
            }
            StateError::DbError(e) => {
                // Log our internal error, but don't reveal error message to Provider.
                log::warn!(
                    "Internal error while updating Agreement state. Error: {}",
                    e
                );
                RemoteAgreementError::InternalError(msg.agreement_id.clone())
            }
        })?;

    notifier.notify(&msg.agreement_id).await;
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
            log::info!("Received proposal from matcher. Adding to events queue.");

            // Add proposal to database together with Negotiation record.
            let proposal = Proposal::new_requestor(proposal.demand, proposal.offer);
            let proposal = db
                .as_dao::<ProposalDao>()
                .save_initial_proposal(proposal)
                .await?;

            // Create Proposal Event and add it to queue (database).
            let subscription_id = proposal.negotiation.subscription_id.clone();
            db.as_dao::<EventsDao>()
                .add_proposal_event(proposal, OwnerType::Requestor)
                .await?;

            // Send channel message to wake all query_events waiting for proposals.
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
