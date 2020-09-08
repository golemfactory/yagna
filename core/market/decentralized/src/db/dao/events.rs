use chrono::Utc;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use thiserror::Error;

use ya_persistence::executor::ConnType;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::db::dao::demand::{demand_status, DemandState};
use crate::db::dao::offer::{query_state, OfferState};
use crate::db::dao::sql_functions::datetime;
use crate::db::model::{Agreement, MarketEvent, OwnerType, Proposal, SubscriptionId};
use crate::db::schema::market_event::dsl;
use crate::db::{DbError, DbResult};
use crate::market::EnvConfig;

const EVENT_STORE_DAYS: EnvConfig<'static, u64> = EnvConfig {
    name: "YAGNA_MARKET_EVENT_STORE_DAYS",
    default: 1, // days
    min: 1,     // days
};

#[derive(Error, Debug)]
pub enum TakeEventsError {
    #[error("Subscription [{0}] not found. Could be unsubscribed.")]
    SubscriptionNotFound(SubscriptionId),
    #[error("Subscription [{0}] expired.")]
    SubscriptionExpired(SubscriptionId),
    #[error("Failed to get events from database. Error: {0}.")]
    DatabaseError(DbError),
}

pub struct EventsDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for EventsDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> EventsDao<'c> {
    pub async fn add_proposal_event(&self, proposal: Proposal, owner: OwnerType) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let event = MarketEvent::from_proposal(proposal, owner);
            diesel::insert_into(dsl::market_event)
                .values(event)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn add_agreement_event(&self, agreement: &Agreement) -> DbResult<()> {
        let event = MarketEvent::from_agreement(agreement);
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_event)
                .values(event)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn take_events(
        &self,
        subscription_id: &SubscriptionId,
        max_events: i32,
        owner: OwnerType,
    ) -> Result<Vec<MarketEvent>, TakeEventsError> {
        let subscription_id = subscription_id.clone();
        do_with_transaction(self.pool, move |conn| {
            // Check subscription wasn't unsubscribed or expired.
            validate_subscription(conn, &subscription_id, owner)?;

            let events = dsl::market_event
                .filter(dsl::subscription_id.eq(&subscription_id))
                .order_by(dsl::timestamp.asc())
                .limit(max_events as i64)
                .load::<MarketEvent>(conn)?;

            // Remove returned events from queue.
            if !events.is_empty() {
                let ids = events.iter().map(|event| event.id).collect::<Vec<_>>();
                diesel::delete(dsl::market_event.filter(dsl::id.eq_any(ids))).execute(conn)?;
            }

            Ok(events)
        })
        .await
    }

    pub async fn remove_events(&self, subscription_id: &SubscriptionId) -> DbResult<()> {
        let subscription_id = subscription_id.clone();
        do_with_transaction(self.pool, move |conn| {
            diesel::delete(dsl::market_event.filter(dsl::subscription_id.eq(&subscription_id)))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn clean(&self) -> DbResult<()> {
        log::debug!("Clean market events: start");
        let interval_days = EVENT_STORE_DAYS.get_value();
        let num_deleted = do_with_transaction(self.pool, move |conn| {
            let nd = diesel::delete(
                dsl::market_event
                    .filter(dsl::timestamp.lt(datetime("NOW", format!("-{} days", interval_days)))),
            )
            .execute(conn)?;
            Result::<usize, DbError>::Ok(nd)
        })
        .await?;
        if num_deleted > 0 {
            log::info!("Clean market events: {} cleaned", num_deleted);
        }
        log::debug!("Clean market events: done");
        Ok(())
    }
}

fn validate_subscription(
    conn: &ConnType,
    subscription_id: &SubscriptionId,
    owner: OwnerType,
) -> Result<(), TakeEventsError> {
    match owner {
        OwnerType::Requestor => match demand_status(conn, &subscription_id)? {
            DemandState::NotFound => Err(TakeEventsError::SubscriptionNotFound(
                subscription_id.clone(),
            ))?,
            DemandState::Expired(_) => Err(TakeEventsError::SubscriptionExpired(
                subscription_id.clone(),
            ))?,
            _ => Ok(()),
        },
        OwnerType::Provider => {
            match query_state(conn, &subscription_id, &Utc::now().naive_utc())? {
                OfferState::NotFound => Err(TakeEventsError::SubscriptionNotFound(
                    subscription_id.clone(),
                ))?,
                OfferState::Expired(_) => Err(TakeEventsError::SubscriptionExpired(
                    subscription_id.clone(),
                ))?,
                _ => Ok(()),
            }
        }
    }
}

impl<ErrorType: Into<DbError>> From<ErrorType> for TakeEventsError {
    fn from(err: ErrorType) -> Self {
        TakeEventsError::DatabaseError(err.into())
    }
}
