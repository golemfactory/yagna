use chrono::NaiveDateTime;
use diesel::{BoolExpressionMethods, ExpressionMethods, QueryDsl, RunQueryDsl};

use ya_client::model::NodeId;
use ya_persistence::executor::readonly_transaction;
use ya_persistence::executor::{AsDao, PoolType};

use crate::db::model::AgreementEvent;
use crate::db::model::AppSessionId;
use crate::db::schema::market_agreement::dsl as agreement;
use crate::db::schema::market_agreement::dsl::market_agreement;
use crate::db::schema::market_agreement_event::dsl as event;
use crate::db::schema::market_agreement_event::dsl::market_agreement_event;
use crate::db::DbResult;

pub struct AgreementEventsDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for AgreementEventsDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
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
        let node_id = node_id.clone();
        readonly_transaction(self.pool, move |conn| {
            // We will get only one Agreement, by using this filter.
            // There will be no way to get Requestor'a Agreement, when being Provider, and vice versa,
            // because AgreementId for Provider and Requestor in Agreement Event table is different.
            let filter_out_not_my_agreements = agreement::provider_id
                .eq(node_id)
                .or(agreement::requestor_id.eq(node_id));

            let mut select_corresponding_agreement = market_agreement
                .select(agreement::id)
                .filter(filter_out_not_my_agreements)
                .into_boxed();

            // If AppSessionId is None, we just select all events, independent from AppSessionId
            // set in Agreement. If AppSessionId is not None we get select only events for this id.
            if let Some(session_id) = session_id {
                select_corresponding_agreement =
                    select_corresponding_agreement.filter(agreement::session_id.eq(session_id));
            };

            Ok(market_agreement_event
                .filter(event::agreement_id.eq_any(select_corresponding_agreement))
                .filter(event::timestamp.gt(after_timestamp))
                .order_by(event::timestamp.asc())
                .limit(max_events as i64)
                .load::<AgreementEvent>(conn)?)
        })
        .await
    }
}
