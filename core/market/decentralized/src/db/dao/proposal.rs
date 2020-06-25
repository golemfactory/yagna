use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::db::models::DbProposal;
use crate::db::models::{Demand as ModelDemand, Negotiation};
use crate::db::models::{Offer as ModelOffer, Proposal};
use crate::db::schema::market_negotiation::dsl as dsl_negotiation;
use crate::db::schema::market_proposal::dsl;
use crate::db::DbResult;

pub struct ProposalDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for ProposalDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> ProposalDao<'c> {
    pub async fn new_initial_proposal(
        &self,
        demand: ModelDemand,
        offer: ModelOffer,
    ) -> DbResult<Proposal> {
        do_with_transaction(self.pool, move |conn| {
            let proposal = DbProposal::new_initial(demand, offer);
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

    pub async fn get_proposal(&self, proposal_id: &str) -> DbResult<Option<Proposal>> {
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
}
