use thiserror::Error;

use ya_persistence::executor::DbExecutor;
use ya_persistence::executor::Error as DbError;

use crate::db::dao::ProposalDao;
use crate::db::model::{Proposal, ProposalId};

#[derive(Error, Debug)]
pub enum GetProposalError {
    #[error("Proposal [{0}] not found.")]
    ProposalNotFound(ProposalId),
    #[error("Failed to get Proposal [{0}]. Error: [{1}]")]
    FailedGetProposal(ProposalId, DbError),
}

pub async fn get_proposal(
    db: &DbExecutor,
    proposal_id: &ProposalId,
) -> Result<Proposal, GetProposalError> {
    Ok(db
        .as_dao::<ProposalDao>()
        .get_proposal(&proposal_id)
        .await
        .map_err(|e| GetProposalError::FailedGetProposal(proposal_id.clone(), e))?
        .ok_or_else(|| GetProposalError::ProposalNotFound(proposal_id.clone()))?)
}
