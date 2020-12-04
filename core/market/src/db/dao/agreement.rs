use chrono::NaiveDateTime;
use diesel::prelude::*;

use ya_client::model::NodeId;
use ya_persistence::executor::{do_with_transaction, AsDao, ConnType, PoolType};

use crate::db::dao::proposal::{has_counter_proposal, set_proposal_accepted};
use crate::db::dao::sql_functions::datetime;
use crate::db::model::{
    Agreement, AgreementEventType, AgreementId, AgreementState, AppSessionId, NewAgreementEvent,
    OwnerType, ProposalId,
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
pub enum StateError {
    #[error("Can't update Agreement [{id}] state from {from} to {to}.")]
    InvalidTransition {
        id: AgreementId,
        from: AgreementState,
        to: AgreementState,
    },
    #[error("Failed to update state. Error: {0}")]
    DbError(DbError),
    #[error("Failed to set AppSessionId. Error: {0}")]
    SessionId(DbError),
    #[error("Failed to add event. Error: {0}")]
    EventError(DbError),
}

impl<'c> AgreementDao<'c> {
    pub async fn select(
        &self,
        id: &AgreementId,
        node_id: Option<NodeId>,
        validation_ts: NaiveDateTime,
    ) -> DbResult<Option<Agreement>> {
        let id = id.clone();
        do_with_transaction(self.pool, move |conn| {
            let mut query = market_agreement.filter(agreement::id.eq(&id)).into_boxed();

            if let Some(node_id) = node_id {
                query = match id.owner() {
                    OwnerType::Provider => query.filter(agreement::provider_id.eq(node_id)),
                    OwnerType::Requestor => query.filter(agreement::requestor_id.eq(node_id)),
                }
            };

            let mut agreement = match query.first::<Agreement>(conn).optional()? {
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

    pub async fn select_by_node(
        &self,
        id: AgreementId,
        node_id: NodeId,
        validation_ts: NaiveDateTime,
    ) -> DbResult<Option<Agreement>> {
        // Because we explicitly disallow agreements between the same identities
        // (i.e. provider_id != requestor_id), we'll always get the right db row
        // with this query.
        let id_swapped = id.clone().swap_owner();
        let id_orig = id.clone();
        do_with_transaction(self.pool, move |conn| {
            let query = market_agreement
                .filter(agreement::id.eq_any(vec![id_orig, id_swapped]))
                .filter(
                    agreement::provider_id
                        .eq(node_id)
                        .or(agreement::requestor_id.eq(node_id)),
                );
            Ok(match query.first::<Agreement>(conn).optional()? {
                Some(mut agreement) => {
                    if agreement.valid_to < validation_ts {
                        agreement.state = AgreementState::Expired;
                        update_state(conn, &id, &agreement.state)?;
                    }
                    Some(agreement)
                }
                None => {
                    log::debug!("Not in DB"); //XXX
                    None
                }
            })
        })
        .await
    }

    pub async fn terminate(
        &self,
        id: &AgreementId,
        reason: Option<String>,
        owner_type: OwnerType,
    ) -> DbResult<bool> {
        let id = id.clone();
        do_with_transaction(self.pool, move |conn| {
            terminate(conn, &id, reason, owner_type)
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

            set_proposal_accepted(conn, &proposal_id)?;
            Ok(agreement)
        })
        .await
    }

    pub async fn confirm(
        &self,
        id: &AgreementId,
        session: &AppSessionId,
    ) -> Result<(), StateError> {
        let id = id.clone();
        let session = session.clone();

        do_with_transaction(self.pool, move |conn| {
            let agreement: Agreement =
                market_agreement.filter(agreement::id.eq(&id)).first(conn)?;

            if agreement.state != AgreementState::Proposal {
                Err(StateError::InvalidTransition {
                    id: id.clone(),
                    from: agreement.state,
                    to: AgreementState::Pending,
                })?
            }

            update_state(conn, &id, &AgreementState::Pending)
                .map_err(|e| StateError::DbError(e.into()))?;

            if let Some(session) = session {
                update_session(conn, &id, &session).map_err(|e| StateError::SessionId(e.into()))?;
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
    ) -> Result<(), StateError> {
        let id = id.clone();
        let session = session.clone();

        do_with_transaction(self.pool, move |conn| {
            let agreement: Agreement =
                market_agreement.filter(agreement::id.eq(&id)).first(conn)?;

            if agreement.state != AgreementState::Pending {
                Err(StateError::InvalidTransition {
                    id: id.clone(),
                    from: agreement.state,
                    to: AgreementState::Approved,
                })?
            }

            update_state(conn, &id, &AgreementState::Approved)
                .map_err(|e| StateError::DbError(e.into()))?;

            // It's important, that if None AppSessionId comes, we shouldn't update Agreement
            // appSessionId field to None. This function can be called in different context, for example
            // on Requestor, when appSessionId is already set.
            if let Some(session) = session {
                update_session(conn, &id, &session).map_err(|e| StateError::SessionId(e.into()))?;
            }

            let event = NewAgreementEvent {
                agreement_id: id.clone(),
                reason: None,
                event_type: AgreementEventType::Approved,
                issuer: OwnerType::Provider, // Always Provider approves.
            };

            diesel::insert_into(market_agreement_event)
                .values(&event)
                .execute(conn)
                .map_err(|e| StateError::EventError(e.into()))?;
            Ok(())
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

impl<ErrorType: Into<DbError>> From<ErrorType> for StateError {
    fn from(err: ErrorType) -> Self {
        StateError::DbError(err.into())
    }
}

impl<ErrorType: Into<DbError>> From<ErrorType> for SaveAgreementError {
    fn from(err: ErrorType) -> Self {
        SaveAgreementError::Internal(err.into())
    }
}

fn update_state(conn: &ConnType, id: &AgreementId, state: &AgreementState) -> DbResult<bool> {
    let num_updated = diesel::update(market_agreement.find(id))
        .set(agreement::state.eq(state))
        .execute(conn)?;
    Ok(num_updated > 0)
}

fn update_session(conn: &ConnType, id: &AgreementId, session: &str) -> DbResult<bool> {
    let num_updated = diesel::update(market_agreement.find(id))
        .set(agreement::session_id.eq(session))
        .execute(conn)?;
    Ok(num_updated > 0)
}

fn terminate(
    conn: &ConnType,
    id: &AgreementId,
    reason: Option<String>,
    owner_type: OwnerType,
) -> DbResult<bool> {
    log::debug!("Termination reason: {:?}", reason);
    let num_updated = diesel::update(agreement::market_agreement.find(id))
        .set(agreement::state.eq(AgreementState::Terminated))
        .execute(conn)?;

    if num_updated == 0 {
        return Ok(false);
    }

    let event = NewAgreementEvent {
        agreement_id: id.clone(),
        reason: reason,
        event_type: AgreementEventType::Terminated,
        issuer: owner_type,
    };

    diesel::insert_into(market_agreement_event)
        .values(&event)
        .execute(conn)?;
    Ok(true)
}
