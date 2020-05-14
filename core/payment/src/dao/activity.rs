use crate::dao::agreement;
use crate::error::DbResult;
use crate::models::activity::WriteObj;
use crate::schema::pay_activity::dsl;
use bigdecimal::{BigDecimal, Zero};
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use std::collections::HashMap;
use ya_client_model::NodeId;
use ya_persistence::executor::{do_with_transaction, AsDao, ConnType, PoolType};
use ya_persistence::types::{BigDecimalField, Role};

pub fn set_amount_due(
    activity_id: &String,
    owner_id: &NodeId,
    total_amount_due: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    let (old_amount, agreement_id): (BigDecimalField, String) = dsl::pay_activity
        .find((activity_id, owner_id))
        .select((dsl::total_amount_due, dsl::agreement_id))
        .first(conn)?;
    let amount_delta = total_amount_due - &old_amount;
    if amount_delta <= BigDecimal::zero().into() {
        return Ok(()); // Debit note with higher amount due already received
    }
    diesel::update(dsl::pay_activity.find((activity_id, owner_id)))
        .set(dsl::total_amount_due.eq(total_amount_due))
        .execute(conn)?;
    agreement::increase_amount_due(&agreement_id, owner_id, &amount_delta, conn)
}

pub fn set_amount_accepted(
    activity_id: &String,
    owner_id: &NodeId,
    total_amount_accepted: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    let (old_amount, agreement_id): (BigDecimalField, String) = dsl::pay_activity
        .find((activity_id, owner_id))
        .select((dsl::total_amount_accepted, dsl::agreement_id))
        .first(conn)?;
    let amount_delta = total_amount_accepted - &old_amount;
    if amount_delta <= BigDecimal::zero().into() {
        return Ok(()); // Debit note with higher amount due already accepted
    }
    diesel::update(dsl::pay_activity.find((activity_id, owner_id)))
        .set(dsl::total_amount_accepted.eq(total_amount_accepted))
        .execute(conn)?;
    agreement::increase_amount_accepted(&agreement_id, owner_id, &amount_delta, conn)
}

pub fn set_amounts_paid(
    amounts: &HashMap<String, BigDecimalField>,
    owner_id: &NodeId,
    conn: &ConnType,
) -> DbResult<()> {
    amounts.iter().try_for_each(|(activity_id, amount)| {
        diesel::update(dsl::pay_activity.find((activity_id, owner_id)))
            .set(dsl::total_amount_paid.eq(amount))
            .execute(conn)
            .map(|_| ())
    })?;
    Ok(())
}

pub struct ActivityDao<'a> {
    pool: &'a PoolType,
}

impl<'a> AsDao<'a> for ActivityDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        Self { pool }
    }
}

impl<'a> ActivityDao<'a> {
    pub async fn create_if_not_exists(
        &self,
        id: String,
        owner_id: NodeId,
        role: Role,
        agreement_id: String,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let existing: Option<String> = dsl::pay_activity
                .find((&id, &owner_id))
                .select(dsl::id)
                .first(conn)
                .optional()?;
            if let Some(_) = existing {
                return Ok(());
            }

            let activity = WriteObj::new(id, owner_id, role, agreement_id);
            diesel::insert_into(dsl::pay_activity)
                .values(activity)
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
