use crate::error::DbResult;
use crate::models::debit_note_event::{ReadObj, WriteObj};
use crate::schema::pay_debit_note_event::dsl as write_dsl;
use crate::schema::pay_debit_note_event_read::dsl as read_dsl;
use chrono::NaiveDateTime;
use diesel::sql_types::{Text, Timestamp};
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

#[derive(Debug, QueryableByName)]
struct DebitNoteEventRow {
    #[sql_type = "Text"]
    role: String,
    #[sql_type = "Text"]
    debit_note_id: String,
    #[sql_type = "Text"]
    owner_id: String,
    #[sql_type = "Text"]
    event_type: String,
    #[sql_type = "Text"]
    timestamp: String,
}

impl<'c> DebitNoteEventDao<'c> {
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

            //get random bool
            let use_sqlite = rand::random::<u64>() % 2 == 0;

            let events: Vec<ReadObj> =
                if let (true, Some(after_timestamp), Some(app_session_id), Some(limit))
                = (use_sqlite, after_timestamp, app_session_id.clone(), max_events) {
                log::info!("Start sqlite Query: {:?} {:?} {:?} {:?}", after_timestamp, node_id, app_session_id, max_events);

                diesel::sql_query(format!(r#"
SELECT pdn.role, pdne.debit_note_id, pdne.owner_id, pdne.event_type, pdne.timestamp, pdne.details, pag.app_session_id FROM pay_debit_note_event AS pdne
JOIN pay_debit_note AS pdn ON pdne.debit_note_id = pdn.id AND pdne.owner_id = pdn.owner_id
JOIN pay_activity AS pac ON pdn.activity_id = pac.id AND pac.owner_id = pdn.owner_id
JOIN pay_agreement AS pag ON pac.agreement_id = pag.id AND pac.owner_id = pag.owner_id
WHERE pdne.owner_id = ? AND pdne.timestamp > ? AND pag.app_session_id = ?
ORDER BY pdne.timestamp ASC
LIMIT {limit};
                "#))
                    .bind::<Text, _>(node_id)
                    .bind::<Timestamp, _>(after_timestamp)
                    .bind::<Text, _>(app_session_id)
                    .load::<ReadObj>(conn)?
            } else {
                log::info!("Start diesel Query: {:?} {:?} {:?} {:?}", after_timestamp, node_id, app_session_id, max_events);

                let mut query = read_dsl::pay_debit_note_event_read
                    .filter(read_dsl::owner_id.eq(node_id))
                    .order_by(read_dsl::timestamp.asc())
                    .into_boxed();
                if let Some(timestamp) = after_timestamp {
                    query = query.filter(read_dsl::timestamp.gt(timestamp.adapt()));
                }
                if let Some(app_session_id) = app_session_id.clone() {
                    query = query.filter(read_dsl::app_session_id.eq(app_session_id));
                }
                if let Some(limit) = max_events {
                    query = query.limit(limit.into());
                }
                query.load(conn)?
            };

            log::info!("End Query: {:?}", events);
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
