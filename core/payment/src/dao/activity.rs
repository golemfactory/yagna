use crate::dao::{agreement, debit_note, debit_note_event};
use crate::error::{DbError, DbResult};
use crate::models::activity::{ReadObj, WriteObj};
use crate::schema::pay_activity::dsl;
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_debit_note::dsl as debit_note_dsl;
use bigdecimal::{BigDecimal, Zero};
use chrono::NaiveDateTime;
use diesel::{
    BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl, RunQueryDsl,
};
use std::collections::HashMap;
use std::str::FromStr;
use ya_client_model::payment::{DebitNoteEventType, DocumentStatus};
use ya_client_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
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

pub fn increase_amount_scheduled(
    activity_id: &String,
    owner_id: &NodeId,
    amount: &BigDecimal,
    conn: &ConnType,
) -> DbResult<()> {
    let activity: WriteObj = dsl::pay_activity
        .find((activity_id, owner_id))
        .first(conn)?;
    let total_amount_scheduled: BigDecimalField =
        (&activity.total_amount_scheduled.0 + amount).into();
    diesel::update(&activity)
        .set(dsl::total_amount_scheduled.eq(total_amount_scheduled))
        .execute(conn)?;
    agreement::increase_amount_scheduled(&activity.agreement_id, owner_id, amount, conn)
}

pub fn increase_amount_paid(
    activity_id: &String,
    owner_id: NodeId,
    amount: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    let (total_amount_paid, agreement_id, role): (BigDecimalField, String, Role) =
        dsl::pay_activity
            .find((activity_id, owner_id))
            .select((dsl::total_amount_paid, dsl::agreement_id, dsl::role))
            .first(conn)?;
    let total_amount_paid = &total_amount_paid + amount;
    diesel::update(dsl::pay_activity.find((activity_id, owner_id)))
        .set(dsl::total_amount_paid.eq(&total_amount_paid))
        .execute(conn)?;

    //extract all debit notes for this activity
    //check if the total amount paid is equal to the total amount due
    //we cannot do that in sql due to lack of decimal support in sqlite
    let debit_note_ids = debit_note_dsl::pay_debit_note
        .filter(debit_note_dsl::activity_id.eq(activity_id))
        .filter(debit_note_dsl::owner_id.eq(owner_id))
        .filter(debit_note_dsl::status.ne_all(vec![
            DocumentStatus::Cancelled.to_string(),
            DocumentStatus::Settled.to_string(),
        ]))
        .select((debit_note_dsl::id, debit_note_dsl::total_amount_due))
        .load::<(String, String)>(conn)?
        .iter()
        .filter_map(|(debit_note_id, total_amount_due)| {
            match BigDecimal::from_str(total_amount_due) {
                Ok(d) => {
                    if total_amount_paid.0 >= d {
                        Some(Ok(debit_note_id.clone()))
                    } else {
                        None
                    }
                }
                Err(e) => Some(Err(DbError::Query(format!(
                    "Error parsing decimal in debit note: {}",
                    e
                )))),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    debit_note::update_status(&debit_note_ids, owner_id, DocumentStatus::Settled, conn)?;

    for debit_note_id in debit_note_ids {
        debit_note_event::create(
            debit_note_id,
            owner_id,
            DebitNoteEventType::DebitNoteSettledEvent,
            conn,
        )?;
    }

    Ok(())
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
    pub async fn get(&self, activity_id: String, owner_id: NodeId) -> DbResult<Option<ReadObj>> {
        readonly_transaction(self.pool, "activity_dao_get", move |conn| {
            let activity = dsl::pay_activity
                .inner_join(
                    agreement_dsl::pay_agreement.on(dsl::owner_id
                        .eq(agreement_dsl::owner_id)
                        .and(dsl::agreement_id.eq(agreement_dsl::id))),
                )
                .select((
                    dsl::id,
                    dsl::owner_id,
                    dsl::role,
                    dsl::agreement_id,
                    dsl::total_amount_due,
                    dsl::total_amount_accepted,
                    dsl::total_amount_scheduled,
                    dsl::total_amount_paid,
                    dsl::created_ts,
                    dsl::updated_ts,
                    agreement_dsl::peer_id,
                    agreement_dsl::payee_addr,
                    agreement_dsl::payer_addr,
                ))
                .filter(dsl::id.eq(&activity_id))
                .filter(dsl::owner_id.eq(&owner_id))
                .first(conn)
                .optional()?;
            Ok(activity)
        })
        .await
    }

    pub async fn list(
        &self,
        role: Option<Role>,
        agreement_id: Option<String>,
    ) -> DbResult<Vec<crate::models::activity::WriteObj>> {
        readonly_transaction(self.pool, "pay_activity_dao_list", move |conn| {
            let mut query = dsl::pay_activity.into_boxed();
            if let Some(agreement_id) = agreement_id {
                query = query.filter(dsl::agreement_id.eq(agreement_id));
            };
            if let Some(role) = role {
                query = query.filter(dsl::role.eq(role));
            };
            let activities = query.load(conn)?;
            Ok(activities.into_iter().collect())
        })
        .await
    }
    pub async fn get_for_node_id(
        &self,
        node_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_items: Option<u32>,
    ) -> DbResult<Vec<WriteObj>> {
        readonly_transaction(self.pool, "activity_dao_get_for_node_id", move |conn| {
            let mut query = dsl::pay_activity.into_boxed();
            query = query.filter(dsl::owner_id.eq(node_id));
            if let Some(date) = after_timestamp {
                query = query.filter(dsl::created_ts.gt(date))
            }
            query = query.order_by(dsl::created_ts.asc());
            if let Some(items) = max_items {
                query = query.limit(items.into())
            }
            let activities: Vec<WriteObj> = query.load(conn)?;
            Ok(activities)
        })
        .await
    }

    pub async fn create_if_not_exists(
        &self,
        id: String,
        owner_id: NodeId,
        role: Role,
        agreement_id: String,
    ) -> DbResult<()> {
        do_with_transaction(
            self.pool,
            "activity_dao_create_if_not_exists",
            move |conn| {
                let existing: Option<String> = dsl::pay_activity
                    .find((&id, &owner_id))
                    .select(dsl::id)
                    .first(conn)
                    .optional()?;
                if existing.is_some() {
                    return Ok(());
                }

                let activity = WriteObj::new(id, owner_id, role, agreement_id);
                diesel::insert_into(dsl::pay_activity)
                    .values(activity)
                    .execute(conn)?;
                Ok(())
            },
        )
        .await
    }
}
