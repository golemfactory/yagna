use crate::error::DbResult;
use crate::models::invoice_event::{ReadObj, WriteObj};
use crate::schema::pay_event_type::dsl as event_type_dsl;
use crate::schema::pay_invoice_event::dsl;
use chrono::NaiveDateTime;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use serde::Serialize;
use std::convert::TryInto;
use ya_client_model::payment::{EventType, InvoiceEvent};
use ya_client_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

pub fn create<T: Serialize>(
    invoice_id: String,
    owner_id: NodeId,
    event_type: EventType,
    details: Option<T>,
    conn: &ConnType,
) -> DbResult<()> {
    let event = WriteObj::new(invoice_id, owner_id, event_type, details)?;
    diesel::insert_into(dsl::pay_invoice_event)
        .values(event)
        .execute(conn)?;
    Ok(())
}

pub struct InvoiceEventDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for InvoiceEventDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> InvoiceEventDao<'c> {
    pub async fn create<T: Serialize>(
        &self,
        invoice_id: String,
        owner_id: NodeId,
        event_type: EventType,
        details: Option<T>,
    ) -> DbResult<()> {
        let event = WriteObj::new(invoice_id, owner_id, event_type, details)?;
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::pay_invoice_event)
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
        _app_session_id: Option<String>,
    ) -> DbResult<Vec<InvoiceEvent>> {
        readonly_transaction(self.pool, move |conn| {
            let mut query = dsl::pay_invoice_event
                .inner_join(event_type_dsl::pay_event_type)
                .filter(dsl::owner_id.eq(node_id))
                .select(crate::schema::pay_invoice_event::all_columns)
                .order_by(dsl::timestamp.asc())
                .into_boxed();
            if let Some(timestamp) = after_timestamp {
                query = query.filter(dsl::timestamp.gt(timestamp));
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
