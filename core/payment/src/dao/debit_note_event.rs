use crate::error::DbResult;
use crate::models::debit_note_event::{ReadObj, WriteObj};
use crate::schema::pay_activity::dsl as activity_dsl;
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_debit_note::dsl as debit_note_dsl;
use crate::schema::pay_debit_note_event::dsl;
use crate::schema::pay_event_type::dsl as event_type_dsl;
use chrono::NaiveDateTime;
use diesel::{BoolExpressionMethods, ExpressionMethods, JoinOnDsl, QueryDsl, RunQueryDsl};
use serde::Serialize;
use std::convert::TryInto;
use ya_client_model::payment::{DebitNoteEvent, EventType};
use ya_client_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

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

    pub async fn get_for_node_id(
        &self,
        node_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_events: Option<u32>,
        app_session_id: Option<String>,
    ) -> DbResult<Vec<DebitNoteEvent>> {
        readonly_transaction(self.pool, move |conn| {
            let mut query = dsl::pay_debit_note_event
                .inner_join(event_type_dsl::pay_event_type)
                .inner_join(
                    debit_note_dsl::pay_debit_note.on(dsl::owner_id
                        .eq(debit_note_dsl::owner_id)
                        .and(dsl::debit_note_id.eq(debit_note_dsl::id))),
                )
                .inner_join(
                    activity_dsl::pay_activity.on(dsl::owner_id
                        .eq(activity_dsl::owner_id)
                        .and(debit_note_dsl::activity_id.eq(activity_dsl::id))),
                )
                .inner_join(
                    agreement_dsl::pay_agreement.on(dsl::owner_id
                        .eq(agreement_dsl::owner_id)
                        .and(activity_dsl::agreement_id.eq(agreement_dsl::id))),
                )
                .filter(dsl::owner_id.eq(node_id))
                .select(crate::schema::pay_debit_note_event::all_columns)
                .order_by(dsl::timestamp.asc())
                .into_boxed();
            if let Some(timestamp) = after_timestamp {
                query = query.filter(dsl::timestamp.gt(timestamp));
            }
            if let Some(app_session_id) = app_session_id {
                query = query.filter(agreement_dsl::app_session_id.eq(app_session_id));
            }
            if let Some(limit) = max_events {
                query = query.limit(limit.into());
            }
            let events: Vec<ReadObj> = query.load(conn)?;
            events.into_iter().map(TryInto::try_into).collect()
        })
        .await
    }
}
