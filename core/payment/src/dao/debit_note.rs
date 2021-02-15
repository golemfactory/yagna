use crate::dao::{activity, debit_note_event};
use crate::error::DbResult;
use crate::models::debit_note::{ReadObj, WriteObj};
use crate::schema::pay_activity::dsl as activity_dsl;
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_debit_note::dsl;
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::{
    self, BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl,
    RunQueryDsl,
};
use std::collections::HashMap;
use std::convert::TryInto;
use ya_client_model::payment::{DebitNote, DebitNoteEventType, DocumentStatus, NewDebitNote};
use ya_client_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::{BigDecimalField, Role};

pub struct DebitNoteDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for DebitNoteDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

// FIXME: This could probably be a function
macro_rules! query {
    () => {
        dsl::pay_debit_note
            .inner_join(
                activity_dsl::pay_activity.on(dsl::owner_id
                    .eq(activity_dsl::owner_id)
                    .and(dsl::activity_id.eq(activity_dsl::id))),
            )
            .inner_join(
                agreement_dsl::pay_agreement.on(dsl::owner_id
                    .eq(agreement_dsl::owner_id)
                    .and(activity_dsl::agreement_id.eq(agreement_dsl::id))),
            )
            .select((
                dsl::id,
                dsl::owner_id,
                dsl::role,
                dsl::previous_debit_note_id,
                dsl::activity_id,
                dsl::status,
                dsl::timestamp,
                dsl::total_amount_due,
                dsl::usage_counter_vector,
                dsl::payment_due_date,
                activity_dsl::agreement_id,
                agreement_dsl::peer_id,
                agreement_dsl::payee_addr,
                agreement_dsl::payer_addr,
                agreement_dsl::payment_platform,
            ))
    };
}

pub fn update_status(
    debit_note_ids: &Vec<String>,
    owner_id: &NodeId,
    status: &DocumentStatus,
    conn: &ConnType,
) -> DbResult<()> {
    diesel::update(
        dsl::pay_debit_note
            .filter(dsl::id.eq_any(debit_note_ids))
            .filter(dsl::owner_id.eq(owner_id)),
    )
    .set(dsl::status.eq(status.to_string()))
    .execute(conn)?;
    Ok(())
}

pub fn get_paid_amount_per_activity(
    debit_note_ids: &Vec<String>,
    owner_id: &NodeId,
    conn: &ConnType,
) -> DbResult<HashMap<String, BigDecimalField>> {
    // This method is equivalent to the following query:
    // SELECT (activity_id, MAX(amount))
    // FROM pay_debit_note
    // GROUP BY activity_id
    // WHERE id IN debit_note_ids
    // Cannot be done by SQL because amounts are stored as text.

    let debit_note_amounts: Vec<(String, BigDecimalField)> = dsl::pay_debit_note
        .filter(
            dsl::id
                .eq_any(debit_note_ids)
                .and(dsl::owner_id.eq(owner_id)),
        )
        .select((dsl::activity_id, dsl::total_amount_due))
        .load(conn)?;
    let activity_amounts =
        debit_note_amounts
            .into_iter()
            .fold(HashMap::new(), |mut map, (activity_id, amount)| {
                let current_amount = map.entry(activity_id).or_default();
                if &amount > current_amount {
                    *current_amount = amount;
                }
                map
            });
    Ok(activity_amounts)
}

impl<'c> DebitNoteDao<'c> {
    pub async fn create_new(
        &self,
        debit_note: NewDebitNote,
        issuer_id: NodeId,
    ) -> DbResult<String> {
        do_with_transaction(self.pool, move |conn| {
            let previous_debit_note_id = dsl::pay_debit_note
                .select(dsl::id)
                .filter(dsl::activity_id.eq(&debit_note.activity_id))
                .filter(dsl::owner_id.eq(&issuer_id))
                .order_by(dsl::timestamp.desc())
                .first(conn)
                .optional()?;
            let debit_note = WriteObj::issued(debit_note, previous_debit_note_id, issuer_id);
            let debit_note_id = debit_note.id.clone();
            activity::set_amount_due(
                &debit_note.activity_id,
                &debit_note.owner_id,
                &debit_note.total_amount_due,
                conn,
            )?;
            diesel::insert_into(dsl::pay_debit_note)
                .values(debit_note)
                .execute(conn)?;
            Ok(debit_note_id)
        })
        .await
    }

    pub async fn insert_received(&self, debit_note: DebitNote) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let previous_debit_note_id = dsl::pay_debit_note
                .select(dsl::id)
                .filter(dsl::activity_id.eq(&debit_note.activity_id))
                .order_by(dsl::timestamp.desc())
                .first(conn)
                .optional()?;
            let debit_note = WriteObj::received(debit_note, previous_debit_note_id);
            let debit_note_id = debit_note.id.clone();
            let owner_id = debit_note.owner_id.clone();
            activity::set_amount_due(
                &debit_note.activity_id,
                &debit_note.owner_id,
                &debit_note.total_amount_due,
                conn,
            )?;
            diesel::insert_into(dsl::pay_debit_note)
                .values(debit_note)
                .execute(conn)?;
            debit_note_event::create::<()>(
                debit_note_id,
                owner_id,
                DebitNoteEventType::DebitNoteReceivedEvent,
                None,
                conn,
            )?;
            Ok(())
        })
        .await
    }

    pub async fn get(
        &self,
        debit_note_id: String,
        owner_id: NodeId,
    ) -> DbResult<Option<DebitNote>> {
        readonly_transaction(self.pool, move |conn| {
            let debit_note: Option<ReadObj> = query!()
                .filter(dsl::id.eq(debit_note_id))
                .filter(dsl::owner_id.eq(owner_id))
                .first(conn)
                .optional()?;
            match debit_note {
                Some(debit_note) => Ok(Some(debit_note.try_into()?)),
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn get_all(&self) -> DbResult<Vec<DebitNote>> {
        readonly_transaction(self.pool, move |conn| {
            let debit_notes: Vec<ReadObj> = query!().load(conn)?;
            debit_notes.into_iter().map(TryInto::try_into).collect()
        })
        .await
    }

    pub async fn get_for_node_id(
        &self,
        node_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_items: Option<u32>,
    ) -> DbResult<Vec<DebitNote>> {
        readonly_transaction(self.pool, move |conn| {
            let mut query = query!().filter(dsl::owner_id.eq(node_id)).into_boxed();
            if let Some(date) = after_timestamp {
                query = query.filter(dsl::timestamp.gt(date))
            }
            if let Some(items) = max_items {
                query = query.limit(items.into())
            }
            let debit_notes: Vec<ReadObj> = query.load(conn)?;
            debit_notes.into_iter().map(TryInto::try_into).collect()
        })
        .await
    }

    pub async fn mark_received(&self, debit_note_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::pay_debit_note.find((debit_note_id, owner_id)))
                .set(dsl::status.eq(DocumentStatus::Received.to_string()))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn accept(&self, debit_note_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let (activity_id, amount, role): (String, BigDecimalField, Role) = dsl::pay_debit_note
                .find((&debit_note_id, &owner_id))
                .select((dsl::activity_id, dsl::total_amount_due, dsl::role))
                .first(conn)?;
            let mut events = vec![DebitNoteEventType::DebitNoteAcceptedEvent];

            // Zero-amount debit notes should be settled immediately.
            let status = if amount.0 == BigDecimal::from(0) {
                events.push(DebitNoteEventType::DebitNoteSettledEvent);
                DocumentStatus::Settled
            } else {
                DocumentStatus::Accepted
            };

            update_status(&vec![debit_note_id.clone()], &owner_id, &status, conn)?;
            activity::set_amount_accepted(&activity_id, &owner_id, &amount, conn)?;
            if let Role::Provider = role {
                for event in events {
                    debit_note_event::create::<()>(
                        debit_note_id.clone(),
                        owner_id,
                        event,
                        None,
                        conn,
                    )?;
                }
            }

            Ok(())
        })
        .await
    }

    // TODO: Implement reject debit note
    // pub async fn reject(&self, debit_note_id: String, owner_id: NodeId) -> DbResult<()> {
    //     do_with_transaction(self.pool, move |conn| {
    //         let (activity_id, role): (String, Role) = dsl::pay_debit_note
    //             .find((&debit_note_id, &owner_id))
    //             .select((dsl::activity_id, dsl::role))
    //             .first(conn)?;
    //         update_status(
    //             &vec![debit_note_id.clone()],
    //             &owner_id,
    //             &DocumentStatus::Rejected,
    //             conn,
    //         )?;
    //         if let Role::Provider = role {
    //             debit_note_event::create::<()>(
    //                 debit_note_id,
    //                 owner_id,
    //                 DebitNoteEventType::DebitNoteRejectedEvent,
    //                 None,
    //                 conn,
    //             )?;
    //         }
    //         Ok(())
    //     })
    //     .await
    // }
}
