use std::collections::HashMap;
use std::convert::TryInto;

use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::sqlite::Sqlite;
use diesel::{
    self, BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl,
    RunQueryDsl,
};
use serde_json::Value::Null;
use ya_client_model::payment::{
    DebitNote, DebitNoteEventType, DocumentStatus, NewDebitNote, Rejection,
};
use ya_client_model::NodeId;
use ya_core_model::payment::local::SchedulePayment;

use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::{BigDecimalField, Role};
use ya_persistence::wrap_ro;

use crate::dao::{activity, debit_note_event};
use crate::error::{DbError, DbResult, NotFoundExtension};
use crate::models::debit_note::{ReadObj, WriteObj};
use crate::schema::pay_activity::dsl as activity_dsl;
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_debit_note::dsl;
use crate::utils::response;

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

struct DebitNoteShort {
    activity_id: String,
    role: Role,
    status: DocumentStatus,
    peer_id: NodeId,
}

pub mod raw {
    use super::DebitNoteShort;
    use crate::error::{DbResult, NotFoundExtension};
    use crate::models::debit_note::ReadObj;
    use diesel::prelude::*;
    use std::collections::HashMap;
    use ya_client_model::payment::{DebitNote, DocumentStatus};
    use ya_client_model::NodeId;
    use ya_persistence::executor::{readonly_transaction, ConnType};
    use ya_persistence::types::{BigDecimalField, Role};

    use crate::schema::pay_activity::dsl as activity_dsl;
    use crate::schema::pay_agreement::dsl as agreement_dsl;
    use crate::schema::pay_debit_note::dsl;

    pub(super) fn select_debit_note(
        conn: &ConnType,
        debit_note_id: &str,
        owner_id: &NodeId,
    ) -> DbResult<DebitNoteShort> {
        let (activity_id, role, status, peer_id): (String, Role, String, NodeId) =
            dsl::pay_debit_note
                .find((debit_note_id, owner_id))
                .inner_join(
                    crate::schema::pay_activity::dsl::pay_activity.on(dsl::owner_id
                        .eq(crate::schema::pay_activity::dsl::owner_id)
                        .and(dsl::activity_id.eq(crate::schema::pay_activity::dsl::id))),
                )
                .inner_join(
                    crate::schema::pay_agreement::dsl::pay_agreement.on(dsl::owner_id
                        .eq(crate::schema::pay_agreement::dsl::owner_id)
                        .and(
                            crate::schema::pay_activity::dsl::agreement_id
                                .eq(crate::schema::pay_agreement::dsl::id),
                        )),
                )
                .select((
                    dsl::activity_id,
                    dsl::role,
                    dsl::status,
                    crate::schema::pay_agreement::dsl::peer_id,
                ))
                .get_result(conn)
                .map_err_not_found()?;

        let status: DocumentStatus = status.try_into()?;

        Ok(DebitNoteShort {
            activity_id,
            role,
            status,
            peer_id,
        })
    }

    pub fn update_status(
        conn: &ConnType,
        debit_note_ids: &Vec<String>,
        owner_id: &NodeId,
        status: &DocumentStatus,
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
        let activity_amounts = debit_note_amounts.into_iter().fold(
            HashMap::new(),
            |mut map, (activity_id, amount)| {
                let current_amount = map.entry(activity_id).or_default();
                if &amount > current_amount {
                    *current_amount = amount;
                }
                map
            },
        );
        Ok(activity_amounts)
    }

    pub fn get(
        conn: &ConnType,
        debit_note_id: &str,
        owner_id: &NodeId,
    ) -> DbResult<Option<DebitNote>> {
        let debit_note: Option<ReadObj> = query!()
            .filter(dsl::id.eq(debit_note_id))
            .filter(dsl::owner_id.eq(owner_id))
            .first(conn)
            .optional()?;
        match debit_note {
            Some(debit_note) => Ok(Some(debit_note.try_into()?)),
            None => Ok(None),
        }
    }

    //    <Conn: diesel::Connection<Backend = Sqlite>>
}

impl<'c> DebitNoteDao<'c> {
    pub async fn create_new(
        &self,
        debit_note: NewDebitNote,
        issuer_id: NodeId,
    ) -> DbResult<String> {
        do_with_transaction(self.pool, "debit_note_dao_create_new", move |conn| {
            let previous_debit_note_id = dsl::pay_debit_note
                .select(dsl::id)
                .filter(dsl::activity_id.eq(&debit_note.activity_id))
                .filter(dsl::owner_id.eq(&issuer_id))
                .order_by(dsl::timestamp.desc())
                .first(conn)
                .optional()?;
            let debit_note = WriteObj::issued(debit_note, previous_debit_note_id, issuer_id);
            let debit_note_id = debit_note.id.clone();
            let owner_id = debit_note.owner_id;
            activity::set_amount_due(
                &debit_note.activity_id,
                &debit_note.owner_id,
                &debit_note.total_amount_due,
                conn,
            )?;
            diesel::insert_into(dsl::pay_debit_note)
                .values(debit_note)
                .execute(conn)?;
            debit_note_event::create(
                debit_note_id.clone(),
                owner_id,
                DebitNoteEventType::DebitNoteReceivedEvent,
                conn,
            )?;
            Ok(debit_note_id)
        })
        .await
    }

    pub async fn insert_received(&self, debit_note: DebitNote) -> DbResult<()> {
        do_with_transaction(self.pool, "debit_note_dao_insert_received", move |conn| {
            let previous_debit_note_id = dsl::pay_debit_note
                .select(dsl::id)
                .filter(dsl::activity_id.eq(&debit_note.activity_id))
                .order_by(dsl::timestamp.desc())
                .first(conn)
                .optional()?;
            let debit_note = WriteObj::received(debit_note, previous_debit_note_id);
            let debit_note_id = debit_note.id.clone();
            let owner_id = debit_note.owner_id;
            activity::set_amount_due(
                &debit_note.activity_id,
                &debit_note.owner_id,
                &debit_note.total_amount_due,
                conn,
            )?;
            diesel::insert_into(dsl::pay_debit_note)
                .values(debit_note)
                .execute(conn)?;
            debit_note_event::create(
                debit_note_id,
                owner_id,
                DebitNoteEventType::DebitNoteReceivedEvent,
                conn,
            )?;
            Ok(())
        })
        .await
    }

    wrap_ro! {
        pub async fn get(debit_note_id: String, owner_id: NodeId) -> DbResult<Option<DebitNote>>;
    }

    pub async fn list(
        &self,
        role: Option<Role>,
        status: Option<DocumentStatus>,
        payable: Option<bool>,
    ) -> DbResult<Vec<DebitNote>> {
        readonly_transaction(self.pool, "debit_note_dao_list", move |conn| {
            let mut query = query!().into_boxed();
            if let Some(role) = role {
                query = query.filter(dsl::role.eq(role.to_string()));
            }
            if let Some(status) = status {
                query = query.filter(dsl::status.eq(status.to_string()));
            }
            if let Some(payable) = payable {
                // Payable debit notes have not-null payment_due_date.
                if payable {
                    query = query.filter(dsl::payment_due_date.is_not_null());
                } else {
                    query = query.filter(dsl::payment_due_date.is_null());
                }
            }

            let debit_notes: Vec<ReadObj> = query.order_by(dsl::timestamp.desc()).load(conn)?;
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
        readonly_transaction(self.pool, "debit_note_dao_get_for_node_id", move |conn| {
            let mut query = query!().filter(dsl::owner_id.eq(node_id)).into_boxed();
            if let Some(date) = after_timestamp {
                query = query.filter(dsl::timestamp.gt(date))
            }
            query = query.order_by(dsl::timestamp.asc());
            if let Some(items) = max_items {
                query = query.limit(items.into())
            }
            let debit_notes: Vec<ReadObj> = query.load(conn)?;
            debit_notes.into_iter().map(TryInto::try_into).collect()
        })
        .await
    }

    pub async fn mark_received(&self, debit_note_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, "debit_note_dao_mark_received", move |conn| {
            diesel::update(dsl::pay_debit_note.find((debit_note_id, owner_id)))
                .set(dsl::status.eq(DocumentStatus::Received.to_string()))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn accept_start(
        &self,
        debit_note_id: String,
        owner_id: NodeId,
        total_amount_accepted: BigDecimal,
        allocation_id: &str,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, "debit_note::accept_start", move |conn| {
            let debit_note: ReadObj = query!()
                .filter(dsl::id.eq(debit_note_id))
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::role.eq(Role::Requestor))
                .first(conn)
                .map_err_not_found()?;

            let status: DocumentStatus = debit_note.status.try_into()?;
            match status {
                DocumentStatus::Received | DocumentStatus::Rejected | DocumentStatus::Failed => (),
                DocumentStatus::Accepted | DocumentStatus::Settled => return Ok(()),
                DocumentStatus::Issued => return DbError::bad_request("Illegal status: issued"),
                DocumentStatus::Cancelled => return DbError::bad_request("Debit note cancelled"),
            }

            if debit_note.total_amount_due.0 != total_amount_accepted {
                return DbError::bad_request("Invalid amount accepted");
            }
            let activity_id = debit_note.activity_id.as_str();
            let (activity_accepted, activity_scheduled, activity_version): (
                BigDecimalField,
                BigDecimalField,
                i32,
            ) = activity_dsl::pay_activity
                .find((activity_id, &owner_id))
                .select((
                    activity_dsl::total_amount_accepted,
                    activity_dsl::total_amount_scheduled,
                    activity_dsl::rec_version,
                )).get_result(conn)?;

            let amount_to_accept = total_amount_accepted - activity_accepted.0;
            /*let amount_to_pay = debit_note.payment_due_date.map(|date| {
                SchedulePayment::from_debit_note()
            });*/

            todo!();

            Ok::<_, DbError>(())
        })
        .await
    }

    pub async fn accept(&self, debit_note_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, "debit_note_dao_accept", move |conn| {
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

            // Accept called on provider is invoked by the requestor, meaning the status must by synchronized.
            // For requestor, a separate `mark_accept_sent` call is required to mark synchronization when the information
            // is delivered to the Provider.
            if role == Role::Requestor {
                diesel::update(
                    dsl::pay_debit_note
                        .filter(dsl::id.eq(debit_note_id.clone()))
                        .filter(dsl::owner_id.eq(owner_id)),
                )
                .set(dsl::send_accept.eq(true))
                .execute(conn)?;
            }

            raw::update_status(conn, &vec![debit_note_id.clone()], &owner_id, &status)?;
            activity::set_amount_accepted(&activity_id, &owner_id, &amount, conn)?;
            for event in events {
                debit_note_event::create(debit_note_id.clone(), owner_id, event, conn)?;
            }

            Ok(())
        })
        .await
    }

    /// Mark the debit-note as synchronized with the other side.
    ///
    /// Automatically marks all previous debit notes as accept-sent if that's not already the case.
    ///
    /// Err(_) is only produced by DB issues.
    pub async fn mark_accept_sent(
        &self,
        mut debit_note_id: String,
        owner_id: NodeId,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, "debit_note_mark_accept_sent", move |conn| loop {
            // Mark debit note as accept-sent
            let n = diesel::update(
                dsl::pay_debit_note
                    .filter(dsl::id.eq(debit_note_id.clone()))
                    .filter(dsl::owner_id.eq(owner_id))
                    .filter(dsl::send_accept.eq(true)),
            )
            .set(dsl::send_accept.eq(false))
            .execute(conn)
            .map_err(DbError::from)?;

            // Debit note was already marked as accept-sent
            if n == 0 {
                break Ok(());
            }

            // Get id of previous debit note
            let previous = dsl::pay_debit_note
                .select(dsl::previous_debit_note_id)
                .filter(dsl::id.eq(debit_note_id))
                .filter(dsl::owner_id.eq(owner_id))
                .load::<Option<String>>(conn)?;

            // Continue with the previous debit-note
            if let Some(Some(id)) = previous.first() {
                debit_note_id = id.into();
            } else {
                break Ok(());
            }
        })
        .await
    }

    /// Lists debit notes with send_accept
    pub async fn unsent_accepted(&self, owner_id: NodeId) -> DbResult<Vec<DebitNote>> {
        readonly_transaction(self.pool, "debit_note_unsent_accepted", move |conn| {
            let read: Vec<ReadObj> = query!()
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::send_accept.eq(true))
                .filter(dsl::status.eq(DocumentStatus::Accepted.to_string()))
                .order_by(dsl::timestamp.desc())
                .load(conn)?;
            let mut debit_notes = Vec::new();
            for obj in read {
                debit_notes.push(obj.try_into()?);
            }

            Ok(debit_notes)
        })
        .await
    }

    /// All debit notes with status Issued or Accepted and provider role
    pub async fn dangling(&self, owner_id: NodeId) -> DbResult<Vec<DebitNote>> {
        readonly_transaction(self.pool, "debit_note_dangling", move |conn| {
            let read: Vec<ReadObj> = query!()
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::role.eq(Role::Provider.to_string()))
                .filter(
                    dsl::status
                        .eq(&DocumentStatus::Issued.to_string())
                        .or(dsl::status.eq(&DocumentStatus::Accepted.to_string())),
                )
                .load(conn)?;

            let mut debit_notes = Vec::new();
            for obj in read {
                debit_notes.push(obj.try_into()?);
            }

            Ok(debit_notes)
        })
        .await
    }

    pub async fn cancel(
        &self,
        owner_id: NodeId,
        role: Role,
        debit_note_id: String,
    ) -> DbResult<NodeId> {
        let peer_id = do_with_transaction(self.pool, "cancel_dn", move |conn| -> DbResult<_> {
            let note = raw::select_debit_note(conn, &debit_note_id, &owner_id)?;

            log::info!("got note {}", note.peer_id);

            if !matches!(
                note.status,
                DocumentStatus::Issued | DocumentStatus::Failed | DocumentStatus::Received
            ) {
                return Err(DbError::Integrity(format!(
                    "unable to cancel debit note in state: {}",
                    note.status
                )));
            }

            if note.role != role {
                return Err(DbError::Integrity(format!(
                    "unable to cancel debit note for role {role:?}"
                )));
            }

            let nr = diesel::update(dsl::pay_debit_note)
                .filter(dsl::id.eq(&debit_note_id))
                .filter(dsl::status.eq_any(vec!["ISSUED", "FAILED", "RECEIVED"]))
                .filter(dsl::owner_id.eq(owner_id))
                .set(dsl::status.eq(DocumentStatus::Cancelled.to_string()))
                .execute(conn)?;

            if nr == 0 {
                return Err(DbError::Integrity(
                    "conflict, invalid debit note state".to_string(),
                ));
            }

            // NOTE: There is no index on previous_debit_note_id, that why it is
            // filtered by activity_id.
            let next_notes: i64 = dsl::pay_debit_note
                .filter(dsl::activity_id.eq(&note.activity_id))
                .filter(
                    dsl::previous_debit_note_id
                        .eq(&debit_note_id)
                        .and(dsl::id.ne(&debit_note_id)),
                )
                .count()
                .get_result(conn)?;

            log::info!("next_notes={next_notes}");

            if next_notes != 0 {
                return Err(DbError::Integrity("note has continuation".into()));
            }

            debit_note_event::create(
                debit_note_id,
                owner_id,
                DebitNoteEventType::DebitNoteCancelledEvent,
                conn,
            )?;
            Ok(note.peer_id)
        })
        .await?;

        Ok(peer_id)
    }

    pub async fn mark_reject_recv(
        &self,
        owner_id: NodeId,
        debit_note_id: String,
        peer_id: NodeId,
        rejection: Rejection,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, "mark_reject_dbn", move |conn| {
            let note = raw::select_debit_note(conn, &debit_note_id, &owner_id)?;

            if !matches!(note.role, Role::Provider) {
                return Err(DbError::Integrity("invalid request".to_string()));
            }

            // Already rejected. ignore retry.
            if matches!(note.status, DocumentStatus::Rejected) {
                return Ok(());
            }

            if matches!(note.status, DocumentStatus::Accepted) {
                return Err(DbError::Integrity(format!(
                    "unable to cancel debit note in state: {}",
                    note.status
                )));
            }

            if note.peer_id != peer_id {
                return Err(DbError::Forbidden);
            }

            let nr = diesel::update(dsl::pay_debit_note)
                .filter(dsl::id.eq(&debit_note_id))
                .filter(dsl::status.eq(note.status.to_string()))
                .filter(dsl::owner_id.eq(owner_id))
                .set(dsl::status.eq(DocumentStatus::Rejected.to_string()))
                .execute(conn)?;
            if nr == 0 {
                return Err(DbError::Integrity("concurrent state  change".to_string()));
            }

            debit_note_event::create(
                debit_note_id,
                owner_id,
                DebitNoteEventType::DebitNoteRejectedEvent { rejection },
                conn,
            )?;

            Ok(())
        })
        .await
    }

    pub async fn reject(&self, owner_id: NodeId, debit_note_id: String) -> DbResult<NodeId> {
        do_with_transaction(self.pool, "reject_dbn", move |conn| {
            let note = raw::select_debit_note(conn, &debit_note_id, &owner_id)?;

            if !matches!(
                note.status,
                DocumentStatus::Issued | DocumentStatus::Received
            ) {
                return Err(DbError::Integrity(format!(
                    "unable to reject debit note in state: {}",
                    note.status
                )));
            }

            let nr = diesel::update(dsl::pay_debit_note)
                .filter(dsl::id.eq(&debit_note_id))
                .filter(dsl::status.eq_any(vec!["ISSUED", "RECEIVED"]))
                .filter(dsl::owner_id.eq(owner_id))
                .set(dsl::status.eq(DocumentStatus::Rejected.to_string()))
                .execute(conn)?;

            if nr == 0 {
                return Err(DbError::Integrity(
                    "unable to reject debit, state changed".into(),
                ));
            }

            if matches!(note.role, Role::Provider) {
                debit_note_event::create(
                    debit_note_id,
                    owner_id,
                    DebitNoteEventType::DebitNoteCancelledEvent,
                    conn,
                )?;
            }
            Ok(note.peer_id)
        })
        .await
    }
}
