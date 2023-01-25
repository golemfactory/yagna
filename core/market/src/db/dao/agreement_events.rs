use chrono::NaiveDateTime;
use diesel::{BoolExpressionMethods, ExpressionMethods, QueryDsl, RunQueryDsl};

use ya_client::model::market::Reason;
use ya_client::model::NodeId;
use ya_persistence::executor::PoolType;
use ya_persistence::executor::{readonly_transaction, ConnType};
use ya_persistence::types::AdaptTimestamp;

use crate::db::dao::AgreementDaoError;
use crate::db::model::{Agreement, AgreementEvent, AgreementId, NewAgreementEvent};
use crate::db::model::{AppSessionId, Owner};
use crate::db::schema::market_agreement::dsl as agreement;
use crate::db::schema::market_agreement::dsl::market_agreement;
use crate::db::schema::market_agreement_event::dsl as event;
use crate::db::schema::market_agreement_event::dsl::market_agreement_event;
use crate::db::{AsMixedDao, DbResult};

pub struct AgreementEventsDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsMixedDao<'a> for AgreementEventsDao<'a> {
    fn as_dao(disk_pool: &'a PoolType, _ram_pool: &'a PoolType) -> Self {
        Self { pool: disk_pool }
    }
}

impl<'c> AgreementEventsDao<'c> {
    pub async fn select(
        &self,
        node_id: &NodeId,
        session_id: &AppSessionId,
        max_events: i32,
        after_timestamp: NaiveDateTime,
    ) -> DbResult<Vec<AgreementEvent>> {
        let session_id = session_id.clone();
        let node_id = *node_id;
        readonly_transaction(self.pool, move |conn| {
            // We will get only one Agreement, by using this filter.
            // There will be no way to get Requestor'a Agreement, when being Provider, and vice versa,
            // because AgreementId for Provider and Requestor in Agreement Event table is different.
            let filter_my_agreements = agreement::provider_id
                .eq(node_id)
                .or(agreement::requestor_id.eq(node_id));

            let mut select_corresponding_agreement = market_agreement
                .select(agreement::id)
                .filter(filter_my_agreements)
                .into_boxed();

            // Optionally filter by `AppSessionId`.
            if let Some(session_id) = session_id {
                select_corresponding_agreement =
                    select_corresponding_agreement.filter(agreement::session_id.eq(session_id));
            };

            Ok(market_agreement_event
                .filter(event::agreement_id.eq_any(select_corresponding_agreement))
                .filter(event::timestamp.gt(after_timestamp.adapt()))
                .order_by(event::timestamp.asc())
                .limit(max_events as i64)
                .load::<AgreementEvent>(conn)?)
        })
        .await
    }

    pub async fn select_for_agreement(
        &self,
        agreement_id: &AgreementId,
    ) -> DbResult<Vec<AgreementEvent>> {
        let agreement_id = agreement_id.clone();
        readonly_transaction(self.pool, move |conn| {
            Ok(market_agreement_event
                .filter(event::agreement_id.eq(agreement_id))
                .order_by(event::timestamp.asc())
                .load::<AgreementEvent>(conn)?)
        })
        .await
    }
}

pub(crate) fn create_event(
    conn: &ConnType,
    agreement: &Agreement,
    reason: Option<Reason>,
    terminator: Owner,
    timestamp: NaiveDateTime,
) -> Result<(), AgreementDaoError> {
    let event = NewAgreementEvent::new(agreement, reason, terminator, timestamp)
        .map_err(|e| AgreementDaoError::EventError(e.to_string()))?;

    diesel::insert_into(market_agreement_event)
        .values(&event)
        .execute(conn)
        .map_err(|e| AgreementDaoError::EventError(e.to_string()))?;

    let events = market_agreement_event
        .filter(event::agreement_id.eq(&agreement.id))
        .load::<AgreementEvent>(conn)?;

    for event in events.iter() {
        log::debug!(
            "Event timestamp: {}, type: {}",
            event.timestamp,
            event.event_type
        );
    }

    Ok(())
}
