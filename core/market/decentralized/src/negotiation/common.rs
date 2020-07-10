use thiserror::Error;

use ya_client::model::market::proposal::Proposal as ClientProposal;
use ya_persistence::executor::DbExecutor;
use ya_persistence::executor::Error as DbError;

use crate::db::dao::ProposalDao;
use crate::db::models::Proposal;
use crate::matcher::SubscriptionStore;
use crate::negotiation::ProposalError;
use crate::{ProposalId, SubscriptionId};

type IsInitial = bool;

#[derive(Clone)]
pub struct CommonBroker {
    pub(super) db: DbExecutor,
    pub(super) store: SubscriptionStore,
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
