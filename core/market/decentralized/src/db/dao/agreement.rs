use chrono::NaiveDateTime;
use diesel::prelude::*;

use ya_persistence::executor::{do_with_transaction, AsDao, ConnType, PoolType};

use crate::db::model::{Agreement, AgreementId, AgreementState};
use crate::db::schema::market_agreement::dsl;
use crate::db::{DbError, DbResult};

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
    #[error("Can't update Agreement [{0}] state from {1} to {2}.")]
    InvalidTransition(AgreementId, AgreementState, AgreementState),
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

    pub async fn save(&self, agreement: Agreement) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_agreement)
                .values(&agreement)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn approve(&self, id: &AgreementId) -> Result<(), StateError> {
        let id = id.clone();
        do_with_transaction(self.pool, move |conn| {
            let agreement: Agreement = dsl::market_agreement.filter(dsl::id.eq(&id)).first(conn)?;

            if agreement.state != AgreementState::Pending {
                Err(StateError::InvalidTransition(
                    id.clone(),
                    agreement.state,
                    AgreementState::Approved,
                ))?
            }

            diesel::update(dsl::market_agreement.filter(dsl::id.eq(&id)))
                .set(dsl::state.eq(AgreementState::Approved))
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}

impl<ErrorType: Into<DbError>> From<ErrorType> for StateError {
    fn from(err: ErrorType) -> Self {
        StateError::DbError(err.into())
    }
}

fn update_state(conn: &ConnType, id: &AgreementId, state: &AgreementState) -> DbResult<bool> {
    let num_updated = diesel::update(dsl::market_agreement.find(id))
        .set(dsl::state.eq(state))
        .execute(conn)?;
    Ok(num_updated > 0)
}
