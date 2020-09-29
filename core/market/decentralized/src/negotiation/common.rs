use metrics::counter;
use std::fmt;
use std::str::FromStr;
use std::time::{Duration, Instant};

use ya_client::model::market::proposal::Proposal as ClientProposal;
use ya_client::model::NodeId;
use ya_market_resolver::{match_demand_offer, Match};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::db::dao::{EventsDao, ProposalDao, SaveProposalError};
use crate::db::model::{IssuerType, MarketEvent, OwnerType, Proposal};
use crate::db::model::{ProposalId, SubscriptionId};
use crate::matcher::{
    error::{DemandError, QueryOfferError},
    store::SubscriptionStore,
};
use crate::negotiation::error::GetProposalError;
use crate::negotiation::notifier::NotifierError;
use crate::negotiation::{
    error::{MatchValidationError, ProposalError, QueryEventsError},
    EventNotifier,
};
use crate::protocol::negotiation::error::{CounterProposalError, RemoteProposalError};
use crate::protocol::negotiation::messages::ProposalReceived;

type IsFirst = bool;

#[derive(Clone)]
pub struct CommonBroker {
    pub(super) db: DbExecutor,
    pub(super) store: SubscriptionStore,
    pub(super) notifier: EventNotifier<SubscriptionId>,
}

impl CommonBroker {
    pub async fn counter_proposal(
        &self,
        subscription_id: &SubscriptionId,
        prev_proposal_id: &ProposalId,
        proposal: &ClientProposal,
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
        let max_events = max_events.unwrap_or(i32::max_value());

        if max_events < 0 {
            Err(QueryEventsError::InvalidMaxEvents(max_events))?
        } else if max_events == 0 {
            return Ok(vec![]);
        }

        let mut notifier = self.notifier.listen(subscription_id);
        loop {
            let events = self
                .db
                .as_dao::<EventsDao>()
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
            .as_dao::<EventsDao>()
            .add_proposal_event(proposal, owner)
            .await
            .map_err(|e| {
                // TODO: Don't leak our database error, but send meaningful message as response.
                log::warn!("[ProposalReceived] Error adding Proposal event: {}", e);
                RemoteProposalError::Unexpected(e.to_string())
            })?;

        // Send channel message to wake all query_events waiting for proposals.
        self.notifier.notify(&subscription_id).await;

        counter!("market.proposals.received", 1);
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

pub struct DisplayIdentity<'a>(pub &'a Identity);

impl<'a> fmt::Display for DisplayIdentity<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "'{}' [{}]", &self.0.name, &self.0.identity)
    }
}
