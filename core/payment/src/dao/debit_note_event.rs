use crate::error::DbResult;
use crate::models::debit_note_event::{ReadObj, WriteObj};
use crate::schema::pay_debit_note_event::dsl as write_dsl;
use crate::schema::pay_debit_note_event_read::dsl as read_dsl;
use chrono::NaiveDateTime;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use std::borrow::Cow;
use std::collections::HashSet;
use std::convert::TryInto;
use ya_client_model::payment::{DebitNoteEvent, DebitNoteEventType};
use ya_client_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::{AdaptTimestamp, Role};

pub fn create(
    debit_note_id: String,
    owner_id: NodeId,
    event_type: DebitNoteEventType,
    conn: &ConnType,
) -> DbResult<()> {
    let event = WriteObj::new(debit_note_id, owner_id, event_type)?;
    diesel::insert_into(write_dsl::pay_debit_note_event)
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

impl DebitNoteEventDao<'_> {
    pub async fn create(
        &self,
        debit_note_id: String,
        owner_id: NodeId,
        event_type: DebitNoteEventType,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, "debit_note_create", move |conn| {
            create(debit_note_id, owner_id, event_type, conn)
        })
        .await
    }

    pub async fn get_for_debit_note_id(
        &self,
        debit_note_id: String,
        after_timestamp: Option<NaiveDateTime>,
        max_events: Option<u32>,
        app_session_id: Option<String>,
        requestor_events: Vec<Cow<'static, str>>,
        provider_events: Vec<Cow<'static, str>>,
    ) -> DbResult<Vec<DebitNoteEvent>> {
        readonly_transaction(self.pool, "debit_note_get_for_debit_note_id", move |conn| {
            let mut query = read_dsl::pay_debit_note_event_read
                .filter(read_dsl::debit_note_id.eq(debit_note_id))
                .order_by(read_dsl::timestamp.asc())
                .into_boxed();
            if let Some(timestamp) = after_timestamp {
                query = query.filter(read_dsl::timestamp.gt(timestamp.adapt()));
            }
            if let Some(app_session_id) = app_session_id {
                query = query.filter(read_dsl::app_session_id.eq(app_session_id));
            }
            if let Some(limit) = max_events {
                query = query.limit(limit.into());
            }
            let events: Vec<ReadObj> = query.load(conn)?;
            let requestor_events: HashSet<Cow<'static, str>> =
                requestor_events.into_iter().collect();
            let provider_events: HashSet<Cow<'static, str>> = provider_events.into_iter().collect();
            events
                .into_iter()
                .filter(|e| match e.role {
                    Role::Requestor => requestor_events.contains(e.event_type.as_str()),
                    Role::Provider => provider_events.contains(e.event_type.as_str()),
                })
                .map(TryInto::try_into)
                .collect()
        })
        .await
    }

    pub async fn get_for_node_id(
        &self,
        node_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_events: Option<u32>,
        app_session_id: Option<String>,
        requestor_events: Vec<Cow<'static, str>>,
        provider_events: Vec<Cow<'static, str>>,
    ) -> DbResult<Vec<DebitNoteEvent>> {
        readonly_transaction(self.pool, "debit_note_get_for_node_id", move |conn| {
            let mut query = read_dsl::pay_debit_note_event_read
                .filter(read_dsl::owner_id.eq(node_id))
                .order_by(read_dsl::timestamp.asc())
                .into_boxed();
            if let Some(timestamp) = after_timestamp {
                query = query.filter(read_dsl::timestamp.gt(timestamp.adapt()));
            }
            if let Some(app_session_id) = app_session_id {
                query = query.filter(read_dsl::app_session_id.eq(app_session_id));
            }
            if let Some(limit) = max_events {
                query = query.limit(limit.into());
            }
            let events: Vec<ReadObj> = query.load(conn)?;
            let requestor_events: HashSet<Cow<'static, str>> =
                requestor_events.into_iter().collect();
            let provider_events: HashSet<Cow<'static, str>> = provider_events.into_iter().collect();
            events
                .into_iter()
                .filter(|e| match e.role {
                    Role::Requestor => requestor_events.contains(e.event_type.as_str()),
                    Role::Provider => provider_events.contains(e.event_type.as_str()),
                })
                .map(TryInto::try_into)
                .collect()
        })
        .await
    }
}
