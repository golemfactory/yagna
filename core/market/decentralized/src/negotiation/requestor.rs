use chrono::{DateTime, Utc};
use futures::stream::StreamExt;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

use ya_client::model::market::event::RequestorEvent;
use ya_client::model::market::proposal::Proposal as ClientProposal;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::{
    dao::{AgreementDao, EventsDao, ProposalDao},
    models::{
        Agreement, AgreementId, Demand as ModelDemand, EventError, OwnerType, Proposal, ProposalId,
        SubscriptionId,
    },
    DbResult,
};
use crate::matcher::{RawProposal, SubscriptionStore};
use crate::negotiation::errors::AgreementError;
use crate::negotiation::{notifier::NotifierError, ProposalError};
use crate::protocol::negotiation::{
    messages::{AgreementApproved, AgreementRejected, ProposalReceived, ProposalRejected},
    requestor::NegotiationApi,
};

use super::common::get_proposal;
use super::errors::{NegotiationError, NegotiationInitError, QueryEventsError};
use super::EventNotifier;
use crate::db::models::AgreementState;

/// Requestor part of negotiation logic.
pub struct RequestorBroker {
    api: NegotiationApi,
    db: DbExecutor,
    _store: SubscriptionStore,
    pub notifier: EventNotifier,
}

impl RequestorBroker {
    pub fn new(
        db: DbExecutor,
        _store: SubscriptionStore,
        proposal_receiver: UnboundedReceiver<RawProposal>,
    ) -> Result<RequestorBroker, NegotiationInitError> {
        let api = NegotiationApi::new(
            move |_caller: String, _msg: ProposalReceived| async move { unimplemented!() },
            move |_caller: String, _msg: ProposalRejected| async move { unimplemented!() },
            move |_caller: String, _msg: AgreementApproved| async move { unimplemented!() },
            move |_caller: String, _msg: AgreementRejected| async move { unimplemented!() },
        );

        let notifier = EventNotifier::new();
        let engine = RequestorBroker {
            api,
            db: db.clone(),
            _store,
            notifier: notifier.clone(),
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
        self.notifier.stop_notifying(demand_id).await;

        // We can ignore error, if removing events failed, because they will be never
        // queried again and don't collide with other subscriptions.
        let _ = self
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
        // TODO: Everything should happen under transaction.
        // TODO: Check if subscription is active
        // TODO: Check if this proposal wasn't already countered.
        let prev_proposal = get_proposal(&self.db, prev_proposal_id)
            .await
            .map_err(|e| ProposalError::from(&demand_id, e))?;

        if &prev_proposal.negotiation.subscription_id != demand_id {
            Err(ProposalError::ProposalNotFound(
                prev_proposal_id.clone(),
                demand_id.clone(),
            ))?
        }

        let is_initial = prev_proposal.body.prev_proposal_id.is_none();
        let new_proposal = prev_proposal.counter_with(proposal);
        let proposal_id = new_proposal.body.id.clone();
        self.db
            .as_dao::<ProposalDao>()
            .save_proposal(&new_proposal)
            .await
            .map_err(|e| ProposalError::FailedSaveProposal(prev_proposal_id.clone(), e))?;

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
        let mut timeout = Duration::from_secs_f32(timeout.max(0.0));
        let stop_time = Instant::now() + timeout;
        let max_events = max_events.unwrap_or(i32::max_value());

        if max_events < 0 {
            Err(QueryEventsError::InvalidMaxEvents(max_events))?
        } else if max_events == 0 {
            return Ok(vec![]);
        }

        loop {
            let events = get_events_from_db(&self.db, demand_id, max_events).await?;
            if events.len() > 0 {
                return Ok(events);
            }

            // Solves panic 'supplied instant is later than self'.
            if stop_time < Instant::now() {
                return Ok(vec![]);
            }
            timeout = stop_time - Instant::now();

            if let Err(error) = self
                .notifier
                .wait_for_event_with_timeout(demand_id, timeout)
                .await
            {
                return match error {
                    NotifierError::Timeout(_) => Ok(vec![]),
                    NotifierError::ChannelClosed(_) => {
                        Err(QueryEventsError::InternalError(format!("{}", error)))
                    }
                    NotifierError::Unsubscribed(id) => Err(QueryEventsError::Unsubscribed(id)),
                };
            }
            // Ok result means, that event with required subscription id was added.
            // We can go to next loop to get this event from db. But still we aren't sure
            // that list won't be empty, because other query_events calls can wait for the same event.
        }
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
        let offer_proposal = get_proposal(&self.db, offer_proposal_id)
            .await
            .map_err(|e| AgreementError::from(proposal_id, e))?;

        let demand_proposal_id = offer_proposal
            .body
            .prev_proposal_id
            .clone()
            .ok_or_else(|| AgreementError::NoNegotiations(offer_proposal_id.clone()))?;
        let demand_proposal = get_proposal(&self.db, &demand_proposal_id)
            .await
            .map_err(|e| AgreementError::from(proposal_id, e))?;

        let agreement = Agreement::new(
            demand_proposal,
            offer_proposal,
            valid_to.naive_utc(),
            OwnerType::Requestor,
        );
        let id = agreement.id.clone();
        self.db
            .as_dao::<AgreementDao>()
            .save(agreement)
            .await
            .map_err(|e| AgreementError::Save(proposal_id.clone(), e))?;
        Ok(id)
    }

    /// Signs (not yet) Agreement self-created via `create_agreement`
    /// and sends it to the Provider.
    pub async fn confirm_agreement(
        &self,
        _id: Identity,
        agreement_id: &AgreementId,
    ) -> Result<(), AgreementError> {
        let dao = self.db.as_dao::<AgreementDao>();
        let agreement = match dao
            .select(agreement_id, Utc::now().naive_utc())
            .await
            .map_err(|e| AgreementError::Get(agreement_id.clone(), e))?
        {
            None => return Err(AgreementError::NotFound(agreement_id.clone())),
            Some(agreement) => agreement,
        };

        if agreement.state == AgreementState::Proposal {
            self.api.propose_agreement(agreement).await?;
            dao.update_state(agreement_id, AgreementState::Pending)
                .await
                .map_err(|e| AgreementError::Get(agreement_id.clone(), e))?;
            return Ok(());
        }

        Err(match agreement.state {
            AgreementState::Proposal => panic!("should not happen"),
            AgreementState::Pending => AgreementError::Confirmed(agreement.id),
            AgreementState::Cancelled => AgreementError::Cancelled(agreement.id),
            AgreementState::Rejected => AgreementError::Rejected(agreement.id),
            AgreementState::Approved => AgreementError::Approved(agreement.id),
            AgreementState::Expired => AgreementError::Expired(agreement.id),
            AgreementState::Terminated => AgreementError::Terminated(agreement.id),
        })
    }
}

async fn get_events_from_db(
    db: &DbExecutor,
    demand_id: &SubscriptionId,
    max_events: i32,
) -> Result<Vec<RequestorEvent>, QueryEventsError> {
    let events = db
        .as_dao::<EventsDao>()
        .take_events(demand_id, max_events, OwnerType::Requestor)
        .await?;

    // Map model events to client RequestorEvent.
    let results = futures::stream::iter(events)
        .then(|event| event.into_client_requestor_event(&db))
        .collect::<Vec<Result<RequestorEvent, EventError>>>()
        .await;

    // Filter errors. Can we do something better with errors, than logging them?
    Ok(results
        .into_iter()
        .inspect(|result| {
            if let Err(error) = result {
                log::warn!("Error converting event to client type: {}", error);
            }
        })
        .filter_map(|event| event.ok())
        .collect::<Vec<RequestorEvent>>())
}

pub async fn proposal_receiver_thread(
    db: DbExecutor,
    mut proposal_receiver: UnboundedReceiver<RawProposal>,
    notifier: EventNotifier,
) {
    while let Some(proposal) = proposal_receiver.recv().await {
        let db = db.clone();
        let notifier = notifier.clone();
        match async move {
            log::info!("Received proposal from matcher. Adding to events queue.");

            // Add proposal to database together with Negotiation record.
            let proposal = Proposal::new_initial(proposal.demand, proposal.offer);
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
