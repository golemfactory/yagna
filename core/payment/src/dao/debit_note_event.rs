use crate::error::DbResult;
use crate::models::debit_note_event::{ReadObj, WriteObj};
use crate::schema::pay_debit_note_event::dsl;
use crate::schema::pay_event_type::dsl as event_type_dsl;
use chrono::NaiveDateTime;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use serde::Serialize;
use std::convert::TryInto;
use ya_client_model::payment::{DebitNoteEvent, EventType};
use ya_core_model::ethaddr::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::Role;

pub fn create<T: Serialize>(
    debit_note_id: String,
    owner_id: NodeId,
    event_type: EventType,
    details: Option<T>,
    conn: &ConnType,
) -> DbResult<()> {
    let event = WriteObj::new(debit_note_id, owner_id, event_type, details)?;
    diesel::insert_into(dsl::pay_debit_note_event)
        .values(event)
        .execute(conn)?;
    Ok(())
}

pub struct DebitNoteEventDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for DebitNoteEventDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> DebitNoteEventDao<'c> {
    pub async fn create<T: Serialize>(
        &self,
        debit_note_id: String,
        owner_id: NodeId,
        event_type: EventType,
        details: Option<T>,
    ) -> DbResult<()> {
        let event = WriteObj::new(debit_note_id, owner_id, event_type, details)?;
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::pay_debit_note_event)
                .values(event)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    async fn get_for_role(
        &self,
        node_id: NodeId,
        later_than: Option<NaiveDateTime>,
        role: Role,
    ) -> DbResult<Vec<DebitNoteEvent>> {
        readonly_transaction(self.pool, move |conn| {
            let query = dsl::pay_debit_note_event
                .inner_join(event_type_dsl::pay_event_type)
                .filter(dsl::owner_id.eq(node_id))
                .filter(event_type_dsl::role.eq(role))
                .select(crate::schema::pay_debit_note_event::all_columns)
                .order_by(dsl::timestamp.asc());
            let events: Vec<ReadObj> = match later_than {
                Some(timestamp) => query.filter(dsl::timestamp.gt(timestamp)).load(conn)?,
                None => query.load(conn)?,
            };
            events.into_iter().map(TryInto::try_into).collect()
        })
        .await
    }

    pub async fn get_for_requestor(
        &self,
        node_id: NodeId,
        later_than: Option<NaiveDateTime>,
    ) -> DbResult<Vec<DebitNoteEvent>> {
        self.get_for_role(node_id, later_than, Role::Requestor)
            .await
    }

    pub async fn get_for_provider(
        &self,
        node_id: NodeId,
        later_than: Option<NaiveDateTime>,
    ) -> DbResult<Vec<DebitNoteEvent>> {
        self.get_for_role(node_id, later_than, Role::Provider).await
    }
}
