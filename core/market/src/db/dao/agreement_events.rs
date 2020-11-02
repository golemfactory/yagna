use chrono::NaiveDateTime;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};

use ya_persistence::executor::readonly_transaction;
use ya_persistence::executor::{AsDao, PoolType};

use crate::db::model::AgreementEvent;
use crate::db::model::AppSessionId;
use crate::db::schema::market_agreement_event::dsl;
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
        session_id: &AppSessionId,
        max_events: i32,
        after_timestamp: NaiveDateTime,
    ) -> DbResult<Vec<AgreementEvent>> {
        let session_id = session_id.clone();
        readonly_transaction(self.pool, move |conn| {
            Ok(dsl::market_agreement_event
                .filter(dsl::session_id.eq(&session_id))
                .filter(dsl::timestamp.gt(after_timestamp))
                .order_by(dsl::timestamp.asc())
                .limit(max_events as i64)
                .load::<AgreementEvent>(conn)?)
        })
        .await
    }
}
