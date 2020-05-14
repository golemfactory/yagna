use crate::dao::activity;
use crate::error::DbResult;
use crate::models::debit_note::{ReadObj, WriteObj};
use crate::schema::pay_activity::dsl as activity_dsl;
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_debit_note::dsl;
use diesel::{
    self, BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl,
    RunQueryDsl,
};
use std::collections::HashMap;
use ya_client_model::payment::{DebitNote, InvoiceStatus, NewDebitNote};
use ya_core_model::ethaddr::NodeId;
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
            ))
    };
}

pub fn update_status(
    debit_note_ids: &Vec<String>,
    owner_id: &NodeId,
    status: &InvoiceStatus,
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
            // TODO: Move previous_debit_note_id assignment to database trigger
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
        let debit_note = WriteObj::received(debit_note);
        do_with_transaction(self.pool, move |conn| {
            // TODO: Check previous_debit_note_id
            activity::set_amount_due(
                &debit_note.activity_id,
                &debit_note.owner_id,
                &debit_note.total_amount_due,
                conn,
            )?;
            diesel::insert_into(dsl::pay_debit_note)
                .values(debit_note)
                .execute(conn)?;
            // TODO: Emit event in the same transaction
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
            Ok(debit_note.map(Into::into))
        })
        .await
    }

    pub async fn get_all(&self) -> DbResult<Vec<DebitNote>> {
        readonly_transaction(self.pool, move |conn| {
            let debit_notes: Vec<ReadObj> = query!().load(conn)?;
            Ok(debit_notes.into_iter().map(Into::into).collect())
        })
        .await
    }

    async fn get_for_role(&self, node_id: NodeId, role: Role) -> DbResult<Vec<DebitNote>> {
        readonly_transaction(self.pool, move |conn| {
            let debit_notes: Vec<ReadObj> = query!()
                .filter(dsl::owner_id.eq(node_id))
                .filter(dsl::role.eq(role))
                .load(conn)?;
            Ok(debit_notes.into_iter().map(Into::into).collect())
        })
        .await
    }

    pub async fn get_for_provider(&self, node_id: NodeId) -> DbResult<Vec<DebitNote>> {
        self.get_for_role(node_id, Role::Provider).await
    }

    pub async fn get_for_requestor(&self, node_id: NodeId) -> DbResult<Vec<DebitNote>> {
        self.get_for_role(node_id, Role::Requestor).await
    }

    pub async fn update_status(
        &self,
        debit_note_id: String,
        owner_id: NodeId,
        status: InvoiceStatus,
    ) -> DbResult<()> {
        // TODO: Remove, use specialized methods
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::pay_debit_note.find((debit_note_id, owner_id)))
                .set(dsl::status.eq(status.to_string()))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn accept(&self, debit_note_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let (activity_id, amount): (String, BigDecimalField) = dsl::pay_debit_note
                .find((&debit_note_id, &owner_id))
                .select((dsl::activity_id, dsl::total_amount_due))
                .first(conn)?;
            update_status(
                &vec![debit_note_id],
                &owner_id,
                &InvoiceStatus::Accepted,
                conn,
            )?;
            activity::set_amount_accepted(&activity_id, &owner_id, &amount, conn)?;
            // TODO: Emit event if role == Provider
            Ok(())
        })
        .await
    }
}
