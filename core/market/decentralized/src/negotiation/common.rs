use std::time::{Duration, Instant};
use thiserror::Error;

use ya_client::model::market::proposal::Proposal as ClientProposal;
use ya_persistence::executor::DbExecutor;
use ya_persistence::executor::Error as DbError;

use crate::db::dao::{EventsDao, ProposalDao};
use crate::db::models::{MarketEvent, OwnerType, Proposal};
use crate::matcher::SubscriptionStore;
use crate::negotiation::notifier::NotifierError;
use crate::negotiation::{EventNotifier, ProposalError, QueryEventsError};
use crate::{ProposalId, SubscriptionId};

type IsInitial = bool;

#[derive(Clone)]
pub struct CommonBroker {
    pub(super) db: DbExecutor,
    pub(super) store: SubscriptionStore,
    pub(super) notifier: EventNotifier,
}

impl CommonBroker {
    pub async fn counter_proposal(
        &self,
        subscription_id: &SubscriptionId,
        prev_proposal_id: &ProposalId,
        proposal: &ClientProposal,
    ) -> Result<(Proposal, IsInitial), ProposalError> {
        // TODO: Everything should happen under transaction.
        // TODO: Check if subscription is active
        // TODO: Check if this proposal wasn't already countered.
        let prev_proposal = self
            .get_proposal(prev_proposal_id)
            .await
            .map_err(|e| ProposalError::from(&subscription_id, e))?;

        if &prev_proposal.negotiation.subscription_id != subscription_id {
            Err(ProposalError::ProposalNotFound(
                prev_proposal_id.clone(),
                subscription_id.clone(),
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
        Ok((new_proposal, is_initial))
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

        loop {
            let events = self
                .db
                .as_dao::<EventsDao>()
                .take_events(subscription_id, max_events, owner.clone())
                .await?;

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
                .wait_for_event_with_timeout(subscription_id, timeout)
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

    pub async fn get_proposal(
        &self,
        proposal_id: &ProposalId,
    ) -> Result<Proposal, GetProposalError> {
        Ok(self
            .db
            .as_dao::<ProposalDao>()
            .get_proposal(&proposal_id)
            .await
            .map_err(|e| GetProposalError::FailedGetProposal(proposal_id.clone(), e))?
            .ok_or_else(|| GetProposalError::ProposalNotFound(proposal_id.clone()))?)
    }
}

#[derive(Error, Debug)]
pub enum GetProposalError {
    #[error("Proposal [{0}] not found.")]
    ProposalNotFound(ProposalId),
    #[error("Failed to get Proposal [{0}]. Error: [{1}]")]
    FailedGetProposal(ProposalId, DbError),
}
