use crate::error::DbResult;
use crate::models::debit_note_event::{ReadObj, WriteObj};
use crate::schema::pay_debit_note_event::dsl as write_dsl;
use crate::schema::pay_debit_note_event_read::dsl as read_dsl;
use chrono::NaiveDateTime;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use serde::Serialize;
use std::borrow::Cow;
use std::convert::TryInto;
use ya_client_model::payment::{DebitNoteEvent, DebitNoteEventType};
use ya_client_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

pub fn create<T: Serialize>(
    debit_note_id: String,
    owner_id: NodeId,
    event_type: DebitNoteEventType,
    details: Option<T>,
    conn: &ConnType,
) -> DbResult<()> {
    let event = WriteObj::new(debit_note_id, owner_id, event_type, details)?;
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

impl<'c> DebitNoteEventDao<'c> {
    pub async fn create<T: Serialize + Send + 'static>(
        &self,
        debit_note_id: String,
        owner_id: NodeId,
        event_type: DebitNoteEventType,
        details: Option<T>,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            create(debit_note_id, owner_id, event_type, details, conn)
        })
        .await
    }

    pub async fn get_for_node_id(
        &self,
        node_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_events: Option<u32>,
        app_session_id: Option<String>,
        _requestor_events: Vec<Cow<'static, str>>,
        _provider_events: Vec<Cow<'static, str>>,
    ) -> DbResult<Vec<DebitNoteEvent>> {
        // TODO: filter out by _requestor_events, _provider_events
        readonly_transaction(self.pool, move |conn| {
            let mut query = read_dsl::pay_debit_note_event_read
                .filter(read_dsl::owner_id.eq(node_id))
                .order_by(read_dsl::timestamp.asc())
                .into_boxed();
            if let Some(timestamp) = after_timestamp {
                query = query.filter(read_dsl::timestamp.gt(timestamp));
            }
            if let Some(app_session_id) = app_session_id {
                query = query.filter(read_dsl::app_session_id.eq(app_session_id));
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
