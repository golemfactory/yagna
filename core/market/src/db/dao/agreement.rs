use chrono::NaiveDateTime;
use diesel::prelude::*;

use ya_client::model::NodeId;
use ya_persistence::executor::{do_with_transaction, AsDao, ConnType, PoolType};

use crate::db::dao::agreement_events::create_event;
use crate::db::dao::proposal::{has_counter_proposal, update_proposal_state};
use crate::db::dao::sql_functions::datetime;
use crate::db::model::{
    check_transition, Agreement, AgreementId, AgreementState, AppSessionId, Owner, ProposalId,
    ProposalIdParseError, ProposalState,
};
use crate::db::schema::market_agreement::dsl as agreement;
use crate::db::schema::market_agreement::dsl::market_agreement;
use crate::db::schema::market_agreement_event::dsl as event;
use crate::db::schema::market_agreement_event::dsl::market_agreement_event;
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
    #[error("Can't create second Agreement [{0}] for Proposal [{1}].")]
    Exists(AgreementId, ProposalId),
    #[error("Saving Agreement internal error: {0}.")]
    Internal(DbError),
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
pub enum AgreementDaoError {
    #[error("Can't update Agreement state from {from} to {to}.")]
    InvalidTransition {
        from: AgreementState,
        to: AgreementState,
    },
    #[error("Failed to update state. Error: {0}")]
    DbError(DbError),
    #[error("Failed to set AppSessionId. Error: {0}")]
    SessionId(DbError),
    #[error("Failed to add event. Error: {0}")]
    EventError(String),
    #[error("Invalid Agreement id: {0}")]
    InvalidId(#[from] ProposalIdParseError),
}

impl<'c> AgreementDao<'c> {
    pub async fn select(
        &self,
        id: &AgreementId,
        node_id: Option<NodeId>,
        validation_ts: NaiveDateTime,
    ) -> Result<Option<Agreement>, AgreementDaoError> {
        let id = id.clone();
        do_with_transaction(self.pool, move |conn| {
            let mut query = market_agreement.filter(agreement::id.eq(&id)).into_boxed();

            if let Some(node_id) = node_id {
                query = match id.owner() {
                    Owner::Provider => query.filter(agreement::provider_id.eq(node_id)),
                    Owner::Requestor => query.filter(agreement::requestor_id.eq(node_id)),
                }
            };

            let mut agreement = match query.first::<Agreement>(conn).optional()? {
                None => return Ok(None),
                Some(agreement) => agreement,
            };

            if agreement.valid_to < validation_ts {
                match update_state(conn, &mut agreement, AgreementState::Expired) {
                    // ignore transition errors
                    Err(AgreementDaoError::InvalidTransition { .. }) => Ok(true),
                    r => r,
                }?;
            }

            Ok(Some(agreement))
        })
        .await
    }

    pub async fn select_by_node(
        &self,
        client_agreement_id: &str,
        node_id: NodeId,
        validation_ts: NaiveDateTime,
    ) -> Result<Option<Agreement>, AgreementDaoError> {
        // Because we explicitly disallow agreements between the same identities
        // (i.e. provider_id != requestor_id), we'll always get the right db row
        // with this query.
        let id = AgreementId::from_client(client_agreement_id, Owner::Requestor)?;
        let id_swapped = id.clone().swap_owner();
        do_with_transaction(self.pool, move |conn| {
            let query = market_agreement
                .filter(agreement::id.eq_any(vec![id, id_swapped]))
                .filter(
                    agreement::provider_id
                        .eq(node_id)
                        .or(agreement::requestor_id.eq(node_id)),
                );
            Ok(match query.first::<Agreement>(conn).optional()? {
                Some(mut agreement) => {
                    if agreement.valid_to < validation_ts {
                        match update_state(conn, &mut agreement, AgreementState::Expired) {
                            // ignore transition errors
                            Err(AgreementDaoError::InvalidTransition { .. }) => Ok(true),
                            r => r,
                        }?;
                    }
                    Some(agreement)
                }
                None => None,
            })
        })
        .await
    }

    pub async fn save(&self, agreement: Agreement) -> Result<Agreement, SaveAgreementError> {
        // Agreement is always created for last Provider Proposal.
        let proposal_id = agreement.offer_proposal_id.clone();
        do_with_transaction(self.pool, move |conn| {
            if has_counter_proposal(conn, &proposal_id)? {
                return Err(SaveAgreementError::ProposalCountered(proposal_id.clone()));
            }

            if let Some(agreement) = find_agreement_for_proposal(conn, &proposal_id)? {
                return Err(SaveAgreementError::Exists(
                    agreement.id,
                    proposal_id.clone(),
                ));
            }

            diesel::insert_into(market_agreement)
                .values(&agreement)
                .execute(conn)?;

            update_proposal_state(conn, &proposal_id, ProposalState::Accepted)?;
            Ok(agreement)
        })
        .await
    }

    pub async fn confirm(
        &self,
        id: &AgreementId,
        session: &AppSessionId,
    ) -> Result<(), AgreementDaoError> {
        let id = id.clone();
        let session = session.clone();

        do_with_transaction(self.pool, move |conn| {
            let mut agreement: Agreement =
                market_agreement.filter(agreement::id.eq(&id)).first(conn)?;

            update_state(conn, &mut agreement, AgreementState::Pending)?;

            if let Some(session) = session {
                update_session(conn, &mut agreement, session)?;
            }
            Ok(())
        })
        .await
    }

    /// Function won't change appSessionId, if session parameter is None.
    pub async fn approve(
        &self,
        id: &AgreementId,
        session: &AppSessionId,
    ) -> Result<(), AgreementDaoError> {
        let id = id.clone();
        let session = session.clone();

        do_with_transaction(self.pool, move |conn| {
            let mut agreement: Agreement =
                market_agreement.filter(agreement::id.eq(&id)).first(conn)?;

            update_state(conn, &mut agreement, AgreementState::Approved)?;

            // It's important, that if None AppSessionId comes, we shouldn't update Agreement
            // appSessionId field to None. This function can be called in different context, for example
            // on Requestor, when appSessionId is already set.
            if let Some(session) = session {
                update_session(conn, &mut agreement, session)?;
            }
            Ok(())
        })
        .await
    }

    pub async fn create_approve_event(&self, id: &AgreementId) -> Result<(), AgreementDaoError> {
        let id = id.clone();

        do_with_transaction(self.pool, move |conn| {
            let agreement: Agreement =
                market_agreement.filter(agreement::id.eq(&id)).first(conn)?;

            // Always Provider approves.
            create_event(conn, &agreement, None, Owner::Provider)?;
            Ok(())
        })
        .await
    }

    pub async fn reverse_approve(&self, id: &AgreementId) -> Result<bool, AgreementDaoError> {
        let id = id.clone();

        do_with_transaction(self.pool, move |conn| {
            let num_updated = diesel::update(market_agreement.find(&id))
                .set(agreement::state.eq(&AgreementState::Pending))
                .execute(conn)
                .map_err(|e| AgreementDaoError::DbError(e.into()))?;
            Ok(num_updated > 0)
        })
        .await
    }

    pub async fn terminate(
        &self,
        id: &AgreementId,
        reason: Option<String>,
        terminator: Owner,
    ) -> Result<bool, AgreementDaoError> {
        let id = id.clone();
        do_with_transaction(self.pool, move |conn| {
            log::debug!("Termination reason: {:?}", reason);
            let mut agreement: Agreement =
                market_agreement.filter(agreement::id.eq(&id)).first(conn)?;

            update_state(conn, &mut agreement, AgreementState::Terminated)?;
            create_event(conn, &agreement, reason, terminator)?;

            Ok(true)
        })
        .await
    }

    pub async fn clean(&self) -> DbResult<()> {
        // FIXME use grace time from config file when #460 is merged
        log::trace!("Clean market agreements: start");
        let interval_days = AGREEMENT_STORE_DAYS.get_value();
        let (num_agreements, num_events) = do_with_transaction(self.pool, move |conn| {
            let agreements_to_clean = market_agreement.filter(
                agreement::valid_to.lt(datetime("NOW", format!("-{} days", interval_days))),
            );

            let related_events = market_agreement_event.filter(
                event::agreement_id.eq_any(agreements_to_clean.clone().select(agreement::id)),
            );

            let num_agreements = diesel::delete(agreements_to_clean).execute(conn)?;
            let num_events = diesel::delete(related_events).execute(conn)?;
            Result::<(usize, usize), DbError>::Ok((num_agreements, num_events))
        })
        .await?;

        if num_agreements > 0 {
            log::info!("Cleaned {} market agreements", num_agreements);
            log::info!("Cleaned {} market agreement events", num_events);
        }
        log::trace!("Clean market agreements: done");
        Ok(())
    }
}

fn find_agreement_for_proposal(
    conn: &ConnType,
    proposal_id: &ProposalId,
) -> DbResult<Option<Agreement>> {
    Ok(market_agreement
        .filter(agreement::offer_proposal_id.eq(&proposal_id))
        .first::<Agreement>(conn)
        .optional()?)
}

impl<ErrorType: Into<DbError>> From<ErrorType> for AgreementDaoError {
    fn from(err: ErrorType) -> Self {
        AgreementDaoError::DbError(err.into())
    }
}

impl<ErrorType: Into<DbError>> From<ErrorType> for SaveAgreementError {
    fn from(err: ErrorType) -> Self {
        SaveAgreementError::Internal(err.into())
    }
}

fn update_state(
    conn: &ConnType,
    agreement: &mut Agreement,
    to_state: AgreementState,
) -> Result<bool, AgreementDaoError> {
    check_transition(agreement.state, to_state)?;

    let num_updated = diesel::update(market_agreement.find(&agreement.id))
        .set(agreement::state.eq(&to_state))
        .execute(conn)
        .map_err(|e| AgreementDaoError::DbError(e.into()))?;

    agreement.state = to_state;

    Ok(num_updated > 0)
}

fn update_session(
    conn: &ConnType,
    agreement: &mut Agreement,
    session_id: String,
) -> Result<bool, AgreementDaoError> {
    let num_updated = diesel::update(market_agreement.find(&agreement.id))
        .set(agreement::session_id.eq(&session_id))
        .execute(conn)
        .map_err(|e| AgreementDaoError::SessionId(e.into()))?;
    agreement.session_id = Some(session_id);
    Ok(num_updated > 0)
}
