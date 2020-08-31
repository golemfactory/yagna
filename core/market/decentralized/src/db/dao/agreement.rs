use chrono::NaiveDateTime;
use diesel::prelude::*;

use ya_persistence::executor::{do_with_transaction, AsDao, ConnType, PoolType};

use crate::db::dao::proposal::{has_counter_proposal, set_proposal_accepted};
use crate::db::dao::sql_functions::datetime;
use crate::db::model::{Agreement, AgreementId, AgreementState, ProposalId};
use crate::db::schema::market_agreement::dsl;
use crate::db::{DbError, DbResult};
use crate::market::EnvConfig;

const AGREEMENT_STORE_DAYS: EnvConfig<'static, u64> = EnvConfig {
    name: "YAGNA_MARKET_AGREEMENT_STORE_DAYS",
    default: 90, // days
    min: 30,     // days
};

#[derive(thiserror::Error, Debug)]
pub enum SaveAgreementError {
    #[error("Can't create Agreement for already countered Proposal [{0}].")]
    ProposalCountered(ProposalId),
    #[error("Failed to save Agreement to database. Error: {0}.")]
    DatabaseError(DbError),
}

pub struct AgreementDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for AgreementDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        Self { pool }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum StateError {
    #[error("Can't update Agreement [{id}] state from {from} to {to}.")]
    InvalidTransition {
        id: AgreementId,
        from: AgreementState,
        to: AgreementState,
    },
    #[error("Failed to update state. Error: {0}")]
    DbError(DbError),
}

impl<'c> AgreementDao<'c> {
    pub async fn select(
        &self,
        id: &AgreementId,
        validation_ts: NaiveDateTime,
    ) -> DbResult<Option<Agreement>> {
        let id = id.clone();
        do_with_transaction(self.pool, move |conn| {
            let mut agreement = match dsl::market_agreement
                .filter(dsl::id.eq(&id))
                .first::<Agreement>(conn)
                .optional()?
            {
                None => return Ok(None),
                Some(agreement) => agreement,
            };

            if agreement.valid_to < validation_ts {
                agreement.state = AgreementState::Expired;
                update_state(conn, &id, &agreement.state)?;
            }

            Ok(Some(agreement))
        })
        .await
    }

    pub async fn update_state(&self, id: &AgreementId, state: AgreementState) -> DbResult<bool> {
        let id = id.clone();
        // TODO: sanity check state before changing it
        do_with_transaction(self.pool, move |conn| update_state(conn, &id, &state)).await
    }

    pub async fn save(&self, agreement: Agreement) -> Result<Agreement, SaveAgreementError> {
        // Agreement is always created for last Provider Proposal.
        let proposal_id = agreement.offer_proposal_id.clone();
        do_with_transaction(self.pool, move |conn| {
            if has_counter_proposal(conn, &proposal_id)? {
                return Err(SaveAgreementError::ProposalCountered(proposal_id.clone()));
            }

            diesel::insert_into(dsl::market_agreement)
                .values(&agreement)
                .execute(conn)?;

            set_proposal_accepted(conn, &proposal_id)?;
            Ok(agreement)
        })
        .await
    }

    pub async fn approve(&self, id: &AgreementId) -> Result<(), StateError> {
        let id = id.clone();
        do_with_transaction(self.pool, move |conn| {
            let agreement: Agreement = dsl::market_agreement.filter(dsl::id.eq(&id)).first(conn)?;

            if agreement.state != AgreementState::Pending {
                Err(StateError::InvalidTransition {
                    id: id.clone(),
                    from: agreement.state,
                    to: AgreementState::Approved,
                })?
            }

            diesel::update(dsl::market_agreement.filter(dsl::id.eq(&id)))
                .set(dsl::state.eq(AgreementState::Approved))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn clean(&self) -> DbResult<()> {
        // FIXME use grace time from config file when #460 is merged
        log::debug!("Clean market agreements: start");
        let interval_days = AGREEMENT_STORE_DAYS.get_value();
        let num_deleted = do_with_transaction(self.pool, move |conn| {
            let nd = diesel::delete(
                dsl::market_agreement
                    .filter(dsl::valid_to.lt(datetime("NOW", format!("-{} days", interval_days)))),
            )
            .execute(conn)?;
            Result::<usize, DbError>::Ok(nd)
        })
        .await?;
        if num_deleted > 0 {
            log::info!("Clean market agreements: {} cleaned", num_deleted);
        }
        log::debug!("Clean market agreements: done");
        Ok(())
    }
}

impl<ErrorType: Into<DbError>> From<ErrorType> for StateError {
    fn from(err: ErrorType) -> Self {
        StateError::DbError(err.into())
    }
}

impl<ErrorType: Into<DbError>> From<ErrorType> for SaveAgreementError {
    fn from(err: ErrorType) -> Self {
        SaveAgreementError::DatabaseError(err.into())
    }
}

fn update_state(conn: &ConnType, id: &AgreementId, state: &AgreementState) -> DbResult<bool> {
    let num_updated = diesel::update(dsl::market_agreement.find(id))
        .set(dsl::state.eq(state))
        .execute(conn)?;
    Ok(num_updated > 0)
}
