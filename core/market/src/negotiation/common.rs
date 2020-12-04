use chrono::{DateTime, Utc};
use metrics::counter;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ya_client::model::market::proposal::Proposal as ClientProposal;
use ya_client::model::market::reason::{ConvertReason, JsonReason, Reason};
use ya_client::model::market::NewProposal;
use ya_client::model::NodeId;
use ya_core_model::market::BUS_ID;
use ya_market_resolver::{match_demand_offer, Match};
use ya_net::{self as net, RemoteEndpoint};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::RpcEndpoint;

use crate::config::Config;
use crate::db::dao::{
    AgreementDao, AgreementEventsDao, NegotiationEventsDao, ProposalDao, SaveProposalError,
};
use crate::db::model::{
    Agreement, AgreementEvent, AgreementId, AgreementState, AppSessionId, IssuerType, MarketEvent,
    OwnerType, Proposal,
};
use crate::db::model::{ProposalId, SubscriptionId};
use crate::matcher::{
    error::{DemandError, QueryOfferError},
    store::SubscriptionStore,
};
use crate::negotiation::{
    error::{
        AgreementError, AgreementEventsError, AgreementStateError, GetProposalError,
        MatchValidationError, ProposalError, QueryEventsError, ReasonError,
    },
    notifier::NotifierError,
    EventNotifier,
};
use crate::protocol::negotiation::error::{
    CounterProposalError, GsbAgreementError, RemoteAgreementError, RemoteProposalError,
    TerminateAgreementError,
};
use crate::protocol::negotiation::messages::{
    provider, requestor, AgreementTerminated, ProposalReceived,
};

type IsFirst = bool;

#[derive(Clone)]
pub struct CommonBroker {
    pub(super) db: DbExecutor,
    pub(super) store: SubscriptionStore,
    pub(super) negotiation_notifier: EventNotifier<SubscriptionId>,
    pub(super) session_notifier: EventNotifier<AppSessionId>,
    pub(super) agreement_notifier: EventNotifier<AgreementId>,
    pub(super) config: Arc<Config>,
}

impl CommonBroker {
    pub fn new(
        db: DbExecutor,
        store: SubscriptionStore,
        session_notifier: EventNotifier<AppSessionId>,
        config: Arc<Config>,
    ) -> CommonBroker {
        CommonBroker {
            store,
            db: db.clone(),
            negotiation_notifier: EventNotifier::new(),
            session_notifier,
            agreement_notifier: EventNotifier::new(),
            config,
        }
    }

    pub async fn counter_proposal(
        &self,
        subscription_id: &SubscriptionId,
        prev_proposal_id: &ProposalId,
        proposal: &NewProposal,
        owner: OwnerType,
    ) -> Result<(Proposal, IsFirst), ProposalError> {
        // Check if subscription is still active.
        // Note that subscription can be unsubscribed, before we get to saving
        // Proposal to database. This seems like race conditions, but there's no
        // danger of data inconsistency. If we won't reject countering Proposal here,
        // it will be sent to Provider and his counter Proposal will be rejected later.
        // TODO: We should use validate_subscription function to do this stuff.
        if owner == OwnerType::Provider {
            self.store
                .get_offer(&subscription_id)
                .await
                .map_err(|e| match e {
                    QueryOfferError::Unsubscribed(id) => ProposalError::Unsubscribed(id),
                    QueryOfferError::Expired(id) => ProposalError::SubscriptionExpired(id),
                    QueryOfferError::NotFound(id) => ProposalError::NoSubscription(id),
                    QueryOfferError::Get(..) => {
                        ProposalError::Internal(prev_proposal_id.clone(), e.to_string())
                    }
                })?;
        } else {
            self.store
                .get_demand(&subscription_id)
                .await
                .map_err(|e| match e {
                    DemandError::NotFound(id) => ProposalError::NoSubscription(id),
                    _ => ProposalError::Internal(prev_proposal_id.clone(), e.to_string()),
                })?;
        }

        let prev_proposal = self
            .get_proposal(Some(subscription_id.clone()), prev_proposal_id)
            .await?;

        if prev_proposal.body.issuer == IssuerType::Us {
            return Err(ProposalError::OwnProposal(prev_proposal_id.clone()));
        }

        let is_first = prev_proposal.body.prev_proposal_id.is_none();
        let new_proposal = prev_proposal.from_client(proposal)?;

        validate_match(&new_proposal, &prev_proposal)?;

        self.db
            .as_dao::<ProposalDao>()
            .save_proposal(&new_proposal)
            .await?;
        Ok((new_proposal, is_first))
    }

    pub async fn query_events(
        &self,
        subscription_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
        owner: OwnerType,
    ) -> Result<Vec<MarketEvent>, QueryEventsError> {
        let mut timeout = Duration::from_secs_f32(timeout.max(0.0));
        let stop_time = Instant::now() + timeout;
        let max_events = max_events.unwrap_or(self.config.events.max_events_default);

        if max_events <= 0 || max_events > self.config.events.max_events_max {
            Err(QueryEventsError::InvalidMaxEvents(
                max_events,
                self.config.events.max_events_max,
            ))?
        }

        let mut notifier = self.negotiation_notifier.listen(subscription_id);
        loop {
            let events = self
                .db
                .as_dao::<NegotiationEventsDao>()
                .take_events(subscription_id, max_events, owner)
                .await?;

            if events.len() > 0 {
                return Ok(events);
            }

            // Solves panic 'supplied instant is later than self'.
            if stop_time < Instant::now() {
                return Ok(vec![]);
            }
            timeout = stop_time - Instant::now();

            if let Err(error) = notifier.wait_for_event_with_timeout(timeout).await {
                return match error {
                    NotifierError::Timeout(_) => Ok(vec![]),
                    NotifierError::ChannelClosed(_) => {
                        Err(QueryEventsError::Internal(error.to_string()))
                    }
                    NotifierError::Unsubscribed(id) => Err(QueryEventsError::Unsubscribed(id)),
                };
            }
            // Ok result means, that event with required subscription id was added.
            // We can go to next loop to get this event from db. But still we aren't sure
            // that list won't be empty, because other query_events calls can wait for the same event.
        }
    }

    pub async fn query_agreement_events(
        &self,
        session_id: &AppSessionId,
        timeout: f32,
        max_events: Option<i32>,
        after_timestamp: DateTime<Utc>,
        id: &Identity,
    ) -> Result<Vec<AgreementEvent>, AgreementEventsError> {
        let mut timeout = Duration::from_secs_f32(timeout.max(0.0));
        let stop_time = Instant::now() + timeout;
        let max_events = max_events.unwrap_or(self.config.events.max_events_default);

        if max_events <= 0 || max_events > self.config.events.max_events_max {
            Err(AgreementEventsError::InvalidMaxEvents(
                max_events,
                self.config.events.max_events_max,
            ))?
        }

        let mut agreement_notifier = self.session_notifier.listen(session_id);
        loop {
            let events = self
                .db
                .as_dao::<AgreementEventsDao>()
                .select(
                    &id.identity,
                    session_id,
                    max_events,
                    after_timestamp.naive_utc(),
                )
                .await
                .map_err(|e| AgreementEventsError::Internal(e.to_string()))?;

            if events.len() > 0 {
                counter!("market.agreements.events.queried", events.len() as u64);
                return Ok(events);
            }
            // Solves panic 'supplied instant is later than self'.
            if stop_time < Instant::now() {
                return Ok(vec![]);
            }
            timeout = stop_time - Instant::now();

            if let Err(error) = agreement_notifier
                .wait_for_event_with_timeout(timeout)
                .await
            {
                return match error {
                    NotifierError::Timeout(_) => Ok(vec![]),
                    NotifierError::ChannelClosed(_) => {
                        Err(AgreementEventsError::Internal(error.to_string()))
                    }
                    NotifierError::Unsubscribed(_) => Err(AgreementEventsError::Internal(format!(
                        "Code logic error. Shouldn't get Unsubscribe in Agreement events notifier."
                    ))),
                };
            }
            // Ok result means, that event with required sessionId id was added.
            // We can go to next loop to get this event from db. But still we aren't sure
            // that list won't be empty, because we could get notification for the same appSessionId,
            // but for different identity. Of course we don't return events for other identities,
            // so we will go to sleep again.
        }
    }

    pub async fn get_proposal(
        &self,
        subscription_id: Option<SubscriptionId>,
        id: &ProposalId,
    ) -> Result<Proposal, GetProposalError> {
        Ok(self
            .db
            .as_dao::<ProposalDao>()
            .get_proposal(&id)
            .await
            .map_err(|e| {
                GetProposalError::Internal(id.clone(), subscription_id.clone(), e.to_string())
            })?
            .filter(|proposal| {
                if subscription_id.is_none() {
                    return true;
                }
                let subscription_id = subscription_id.as_ref().unwrap();
                if &proposal.negotiation.subscription_id == subscription_id {
                    return true;
                }
                log::warn!(
                    "Getting Proposal [{}] subscription mismatch; actual: [{}] expected: [{}].",
                    id,
                    proposal.negotiation.subscription_id,
                    subscription_id,
                );
                // We use ProposalNotFound, because we don't want to leak information,
                // that such Proposal exists, but for different subscription_id.
                false
            })
            .ok_or(GetProposalError::NotFound(id.clone(), subscription_id))?)
    }

    pub async fn get_client_proposal(
        &self,
        subscription_id: Option<SubscriptionId>,
        id: &ProposalId,
    ) -> Result<ClientProposal, GetProposalError> {
        self.get_proposal(subscription_id, id)
            .await
            .and_then(|proposal| {
                proposal
                    .into_client()
                    .map_err(|e| GetProposalError::Internal(id.clone(), None, e.to_string()))
            })
    }

    // Called locally via REST
    pub async fn terminate_agreement(
        &self,
        id: Identity,
        agreement_id: AgreementId,
        reason: Option<String>,
    ) -> Result<(), AgreementError> {
        verify_reason(reason.as_ref())?;
        let dao = self.db.as_dao::<AgreementDao>();
        log::debug!(
            "Getting agreement. id: {:?}, agrid: {}, reason: {:?}",
            id,
            agreement_id,
            reason
        ); // XXX
        let mut agreement = match dao
            .select_by_node(
                agreement_id.clone(),
                id.identity.clone(),
                Utc::now().naive_utc(),
            )
            .await
            .map_err(|e| AgreementError::Get(agreement_id.clone(), e))?
        {
            None => return Err(AgreementError::NotFound(agreement_id)),
            Some(agreement) => agreement,
        };
        // from now on agreement_id is invalid. Use only agreement.id
        // (which has valid owner)
        expect_state(&agreement, AgreementState::Approved)?;
        agreement.state = AgreementState::Terminated;
        let owner_type = agreement.id.owner();
        self.propagate_terminate_agreement(
            &agreement,
            id.identity.clone(),
            match owner_type {
                OwnerType::Requestor => agreement.provider_id,
                OwnerType::Provider => agreement.requestor_id,
            },
            reason.clone(),
            owner_type,
        )
        .await?;
        dao.terminate(&agreement.id, reason, owner_type)
            .await
            .map_err(|e| AgreementError::Get(agreement.id.clone(), e))?;

        match owner_type {
            OwnerType::Provider => counter!("market.agreements.provider.terminated", 1),
            OwnerType::Requestor => counter!("market.agreements.requestor.terminated", 1),
        };
        log::info!(
            "Requestor {} terminated Agreement [{}] and sent to Provider.",
            &id.identity,
            &agreement.id,
        );
        Ok(())
    }
    /// Sent to notify other side about termination.
    pub async fn propagate_terminate_agreement(
        &self,
        agreement: &Agreement,
        sender: NodeId,
        receiver: NodeId,
        reason: Option<String>,
        owner_type: OwnerType,
    ) -> Result<(), TerminateAgreementError> {
        let msg = AgreementTerminated {
            agreement_id: agreement.id.clone(),
            reason,
        };
        let provider_service = &provider::agreement_addr(BUS_ID);
        let requestor_service = &requestor::agreement_addr(BUS_ID);
        let service = match owner_type {
            OwnerType::Requestor => provider_service,
            OwnerType::Provider => requestor_service,
        };
        net::from(sender)
            .to(receiver)
            .service(service)
            .send(msg)
            .await
            .map_err(|e| GsbAgreementError(e.to_string(), agreement.id.clone()))??;
        Ok(())
    }

    // Called remotely via GSB
    pub async fn on_agreement_terminated(
        self,
        caller: String,
        msg: AgreementTerminated,
        owner_type: OwnerType,
    ) -> Result<(), TerminateAgreementError> {
        let caller: NodeId =
            caller
                .parse()
                .map_err(|e: ya_client::model::node_id::ParseError| {
                    TerminateAgreementError::CallerParseError {
                        e: e.to_string(),
                        caller,
                        id: msg.agreement_id.clone(),
                    }
                })?;
        Ok(self
            .on_agreement_terminated_inner(caller, msg, owner_type)
            .await?)
    }

    async fn on_agreement_terminated_inner(
        self,
        caller: NodeId,
        msg: AgreementTerminated,
        owner_type: OwnerType,
    ) -> Result<(), RemoteAgreementError> {
        let dao = self.db.as_dao::<AgreementDao>();
        let agreement_id = msg.agreement_id.translate(owner_type);
        let agreement = dao
            .select(&agreement_id, None, Utc::now().naive_utc())
            .await
            .map_err(|_e| RemoteAgreementError::NotFound(agreement_id.clone()))?
            .ok_or(RemoteAgreementError::NotFound(agreement_id.clone()))?;

        match owner_type {
            OwnerType::Requestor => {
                if agreement.provider_id != caller {
                    // Don't reveal, that we know this Agreement id.
                    Err(RemoteAgreementError::NotFound(agreement_id.clone()))?
                }
            }
            OwnerType::Provider => {
                if agreement.requestor_id != caller {
                    // Don't reveal, that we know this Agreement id.
                    Err(RemoteAgreementError::NotFound(agreement_id.clone()))?
                }
            }
        }

        dao.terminate(&agreement_id, msg.reason, owner_type)
            .await
            .map_err(|e| {
                log::info!(
                    "Couldn't terminate agreement. id: {}, e: {}",
                    agreement_id,
                    e
                );
                RemoteAgreementError::InternalError(agreement_id.clone())
            })?;
        Ok(())
    }

    // TODO: We need more elegant solution than this. This function still returns
    //  CounterProposalError, which should be hidden in negotiation API and implementations
    //  of handlers should return RemoteProposalError.
    pub async fn on_proposal_received(
        self,
        caller: String,
        msg: ProposalReceived,
        owner: OwnerType,
    ) -> Result<(), CounterProposalError> {
        let proposal_id = msg.proposal.proposal_id.clone();
        self.proposal_received(caller, msg, owner)
            .await
            .map_err(|e| CounterProposalError::Remote(e, proposal_id))
    }

    pub async fn proposal_received(
        self,
        caller: String,
        msg: ProposalReceived,
        owner: OwnerType,
    ) -> Result<(), RemoteProposalError> {
        // Check if countered Proposal exists.
        let prev_proposal = self
            .get_proposal(None, &msg.prev_proposal_id)
            .await
            .map_err(|_e| RemoteProposalError::NotFound(msg.prev_proposal_id.clone()))?;
        let proposal = prev_proposal.from_draft(msg.proposal);
        proposal.validate_id().map_err(RemoteProposalError::from)?;

        // TODO: do auth
        let _owner_id = NodeId::from_str(&caller)
            .map_err(|e| RemoteProposalError::Unexpected(e.to_string()))?;

        self.validate_subscription(&prev_proposal, owner).await?;
        validate_match(&proposal, &prev_proposal).map_err(RemoteProposalError::NotMatching)?;

        self.db
            .as_dao::<ProposalDao>()
            .save_proposal(&proposal)
            .await
            .map_err(|e| match e {
                SaveProposalError::AlreadyCountered(id) => {
                    RemoteProposalError::AlreadyCountered(id)
                }
                _ => {
                    // TODO: Don't leak our database error, but send meaningful message as response.
                    log::warn!("[ProposalReceived] Error saving Proposal: {}", e);
                    RemoteProposalError::Unexpected(e.to_string())
                }
            })?;

        // Create Proposal Event and add it to queue (database).
        // TODO: If creating Proposal succeeds, but event can't be added, provider
        // TODO: will never answer to this Proposal. Solve problem when Event API will be available.
        let subscription_id = proposal.negotiation.subscription_id.clone();
        let proposal = self
            .db
            .as_dao::<NegotiationEventsDao>()
            .add_proposal_event(proposal, owner)
            .await
            .map_err(|e| {
                // TODO: Don't leak our database error, but send meaningful message as response.
                log::warn!("[ProposalReceived] Error adding Proposal event: {}", e);
                RemoteProposalError::Unexpected(e.to_string())
            })?;

        // Send channel message to wake all query_events waiting for proposals.
        self.negotiation_notifier.notify(&subscription_id).await;

        match owner {
            OwnerType::Requestor => counter!("market.proposals.requestor.received", 1),
            OwnerType::Provider => counter!("market.proposals.provider.received", 1),
        };
        log::info!(
            "Received counter Proposal [{}] for Proposal [{}] from [{}].",
            &proposal.body.id,
            &msg.prev_proposal_id,
            &caller
        );
        Ok(())
    }

    pub async fn validate_subscription(
        &self,
        proposal: &Proposal,
        owner: OwnerType,
    ) -> Result<(), RemoteProposalError> {
        if let Err(e) = self.store.get_offer(&proposal.negotiation.offer_id).await {
            match e {
                QueryOfferError::Unsubscribed(id) => Err(RemoteProposalError::Unsubscribed(id))?,
                QueryOfferError::Expired(id) => Err(RemoteProposalError::Expired(id))?,
                _ => Err(RemoteProposalError::Unexpected(e.to_string()))?,
            }
        };

        // On Requestor side we have both Offer and Demand, but Provider has only Offers.
        if owner == OwnerType::Requestor {
            if let Err(e) = self.store.get_demand(&proposal.negotiation.demand_id).await {
                match e {
                    DemandError::NotFound(id) => Err(RemoteProposalError::Unsubscribed(id))?,
                    _ => Err(RemoteProposalError::Unexpected(e.to_string()))?,
                }
            };
        }
        Ok(())
    }

    pub async fn notify_agreement(&self, agreement: &Agreement) {
        let session_notifier = &self.session_notifier;

        // Notify everyone waiting on Agreement events endpoint.
        if let Some(_) = &agreement.session_id {
            session_notifier.notify(&agreement.session_id.clone()).await;
        }
        // Even if session_id was not None, we want to notify everyone else,
        // that waits without specifying session_id.
        session_notifier.notify(&None).await;

        // This notifies wait_for_agreement endpoint.
        self.agreement_notifier.notify(&agreement.id).await;
    }
}

pub fn validate_match(
    new_proposal: &Proposal,
    prev_proposal: &Proposal,
) -> Result<(), MatchValidationError> {
    match match_demand_offer(
        &new_proposal.body.properties,
        &new_proposal.body.constraints,
        &prev_proposal.body.properties,
        &prev_proposal.body.constraints,
    )
    .map_err(|e| MatchValidationError::MatchingFailed {
        new: new_proposal.body.id.clone(),
        prev: prev_proposal.body.id.clone(),
        error: e.to_string(),
    })? {
        Match::Yes => Ok(()),
        _ => {
            return Err(MatchValidationError::NotMatching {
                new: new_proposal.body.id.clone(),
                prev: prev_proposal.body.id.clone(),
            })
        }
    }
}

pub fn expect_state(
    agreement: &Agreement,
    state: AgreementState,
) -> Result<(), AgreementStateError> {
    if agreement.state == state {
        return Ok(());
    }

    Err(match agreement.state {
        AgreementState::Proposal => AgreementStateError::Proposed(agreement.id.clone()),
        AgreementState::Pending => AgreementStateError::Confirmed(agreement.id.clone()),
        AgreementState::Cancelled => AgreementStateError::Cancelled(agreement.id.clone()),
        AgreementState::Rejected => AgreementStateError::Rejected(agreement.id.clone()),
        AgreementState::Approved => AgreementStateError::Approved(agreement.id.clone()),
        AgreementState::Expired => AgreementStateError::Expired(agreement.id.clone()),
        AgreementState::Terminated => AgreementStateError::Terminated(agreement.id.clone()),
    })?
}

fn verify_reason(reason: Option<&String>) -> Result<(), ReasonError> {
    if let Some(s) = reason {
        Reason::from_json_reason(JsonReason {
            json: serde_json::from_str(s)?,
        })?;
    };
    Ok(())
}
