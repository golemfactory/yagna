#![allow(dead_code)]
use diesel::expression::dsl::now as sql_now;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::db::model::{DbProposal, Negotiation, Proposal, ProposalId};
use crate::db::schema::market_negotiation::dsl as dsl_negotiation;
use crate::db::schema::market_proposal::dsl;
use crate::db::{DbError, DbResult};

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

    pub async fn save_proposal(&self, proposal: &Proposal) -> DbResult<()> {
        let proposal = proposal.body.clone();
        do_with_transaction(self.pool, move |conn| {
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

    pub async fn has_counter_proposal(&self, proposal_id: &ProposalId) -> DbResult<bool> {
        let proposal_id = proposal_id.to_string();
        readonly_transaction(self.pool, move |conn| {
            let proposal: Option<DbProposal> = dsl::market_proposal
                .filter(dsl::prev_proposal_id.eq(&proposal_id))
                .first(conn)
                .optional()?;
            Ok(proposal.is_some())
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
                    .filter(dsl_negotiation::agreement_id.is_null())
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
