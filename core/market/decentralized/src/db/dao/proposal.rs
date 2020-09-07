#![allow(dead_code)]
use diesel::expression::dsl::now as sql_now;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

use crate::db::model::{DbProposal, Negotiation, Proposal, ProposalId, ProposalState};
use crate::db::schema::market_negotiation::dsl as dsl_negotiation;
use crate::db::schema::market_proposal::dsl;
use crate::db::{DbError, DbResult};

#[derive(thiserror::Error, Debug)]
pub enum SaveProposalError {
    #[error("Proposal [{0}] already has counter proposal. Can't counter for the second time.")]
    AlreadyCountered(ProposalId),
    #[error("Failed to save proposal to database. Error: {0}.")]
    DatabaseError(DbError),
    #[error("Proposal [{0}] has no previous proposal. This should not happened when calling save_proposal.")]
    NoPreviousProposal(ProposalId),
}

pub struct ProposalDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for ProposalDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> ProposalDao<'c> {
    pub async fn save_initial_proposal(&self, proposal: Proposal) -> DbResult<Proposal> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl_negotiation::market_negotiation)
                .values(&proposal.negotiation)
                .execute(conn)?;

            diesel::insert_into(dsl::market_proposal)
                .values(&proposal.body)
                .execute(conn)?;
            Ok(proposal)
        })
        .await
    }

    pub async fn save_proposal(&self, proposal: &Proposal) -> Result<(), SaveProposalError> {
        let proposal = proposal.body.clone();
        do_with_transaction(self.pool, move |conn| {
            let prev_proposal = proposal
                .prev_proposal_id
                .clone()
                .ok_or(SaveProposalError::NoPreviousProposal(proposal.id.clone()))?;

            if has_counter_proposal(conn, &prev_proposal)? {
                return Err(SaveProposalError::AlreadyCountered(prev_proposal));
            }

            diesel::insert_into(dsl::market_proposal)
                .values(&proposal)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get_proposal(&self, proposal_id: &ProposalId) -> DbResult<Option<Proposal>> {
        let proposal_id = proposal_id.to_string();
        readonly_transaction(self.pool, move |conn| {
            let proposal: Option<DbProposal> = dsl::market_proposal
                .filter(dsl::id.eq(&proposal_id))
                .first(conn)
                .optional()?;

            let proposal = match proposal {
                Some(proposal) => proposal,
                None => return Ok(None),
            };

            let negotiation: Negotiation = dsl_negotiation::market_negotiation
                .filter(dsl_negotiation::id.eq(&proposal.negotiation_id))
                .first(conn)?;

            Ok(Some(Proposal {
                negotiation,
                body: proposal,
            }))
        })
        .await
    }

    pub async fn clean(&self) -> DbResult<()> {
        // FIXME clean negotiations also
        log::debug!("Clean market proposals: start");
        loop {
            let (num_deleted_p, num_deleted_n) = do_with_transaction(self.pool, move |conn| {
                // diesel forbids the same table appearing more than once in a query
                // so we'll do some manual operations here
                // TODO: Use sql max(expiration_ts)
                let expired_negotiations = dsl_negotiation::market_negotiation
                    .filter(
                        dsl_negotiation::id.ne_all(
                            dsl::market_proposal
                                .filter(dsl::expiration_ts.gt(sql_now))
                                .select(dsl::negotiation_id),
                        ),
                    )
                    .select(dsl_negotiation::id)
                    .load::<String>(conn)?;
                let ndp = diesel::delete(
                    dsl::market_proposal
                        .filter(dsl::negotiation_id.eq_any(expired_negotiations.clone())),
                )
                .execute(conn)?;
                let ndn = diesel::delete(
                    dsl_negotiation::market_negotiation
                        .filter(dsl_negotiation::id.eq_any(expired_negotiations)),
                )
                .execute(conn)?;
                Result::<(usize, usize), DbError>::Ok((ndp, ndn))
            })
            .await?;
            if (num_deleted_p > 0) || (num_deleted_n > 0) {
                log::info!(
                    "Clean market proposals: {}({} negotiations) cleaned",
                    num_deleted_p,
                    num_deleted_n
                );
            } else {
                break;
            }
        }
        log::debug!("Clean market proposals: done");
        Ok(())
    }
}

pub(super) fn has_counter_proposal(conn: &ConnType, proposal_id: &ProposalId) -> DbResult<bool> {
    let proposal: Option<DbProposal> = dsl::market_proposal
        .filter(dsl::prev_proposal_id.eq(&proposal_id))
        .first(conn)
        .optional()?;
    Ok(proposal.is_some())
}

pub(super) fn set_proposal_accepted(conn: &ConnType, proposal_id: &ProposalId) -> DbResult<()> {
    // TODO: Check if we can do transition to this state.
    update_proposal_state(conn, proposal_id, ProposalState::Accepted)
}

fn update_proposal_state(
    conn: &ConnType,
    proposal_id: &ProposalId,
    new_state: ProposalState,
) -> DbResult<()> {
    diesel::update(dsl::market_proposal.filter(dsl::id.eq(&proposal_id)))
        .set(dsl::state.eq(new_state))
        .execute(conn)?;
    Ok(())
}

impl<ErrorType: Into<DbError>> From<ErrorType> for SaveProposalError {
    fn from(err: ErrorType) -> Self {
        SaveProposalError::DatabaseError(err.into())
    }
}
