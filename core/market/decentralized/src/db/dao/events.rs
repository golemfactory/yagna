use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use thiserror::Error;

use ya_persistence::executor::Error as DbError;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::db::dao::demand::{demand_status, DemandState};
use crate::db::models::MarketEvent;
use crate::db::models::{OwnerType, Proposal, SubscriptionId};
use crate::db::schema::market_event::dsl;
use crate::db::DbResult;

#[derive(Error, Debug)]
pub enum TakeEventsError {
    #[error("Removed different number of events '{num_removed}' that expected '{to_remove}'.")]
    EventsRemovalError {
        num_removed: usize,
        to_remove: usize,
    },
    #[error("Subscription [{0}] not found. Could be unsubscribed.")]
    SubscriptionNotFound(SubscriptionId),
    #[error("Subscription [{0}] expired.")]
    SubscriptionExpired(SubscriptionId),
    #[error(transparent)]
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
    pub async fn add_requestor_event(&self, proposal: Proposal) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let event = MarketEvent::from_proposal(&proposal, OwnerType::Requestor);
            diesel::insert_into(dsl::market_event)
                .values(event)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn take_requestor_events(
        &self,
        subscription_id: &SubscriptionId,
        max_events: i32,
    ) -> Result<Vec<MarketEvent>, TakeEventsError> {
        let subscription_id = subscription_id.clone();
        Ok(do_with_transaction(self.pool, move |conn| {
            match demand_status(conn, &subscription_id)? {
                DemandState::NotFound => Err(TakeEventsError::SubscriptionNotFound(
                    subscription_id.clone(),
                ))?,
                DemandState::Expired(_) => Err(TakeEventsError::SubscriptionExpired(
                    subscription_id.clone(),
                ))?,
                _ => (),
            };

            let events = dsl::market_event
                .filter(dsl::subscription_id.eq(&subscription_id))
                .order_by(dsl::timestamp.asc())
                .limit(max_events as i64)
                .load::<MarketEvent>(conn)?;

            // Remove events from queue.
            // We check if events are older, than first events that is the newest
            // since we ordered them ascending.
            // Is it safe?? Can we remove by accident events, that shouldn't be removed?
            let first_event = events.first();
            if let Some(first_event) = first_event {
                let num_removed = diesel::delete(
                    dsl::market_event
                        .filter(dsl::subscription_id.eq(&subscription_id))
                        .filter(dsl::timestamp.le(first_event.timestamp)),
                )
                .execute(conn)?;

                if num_removed != events.len() {
                    let error = TakeEventsError::EventsRemovalError {
                        num_removed,
                        to_remove: events.len(),
                    };
                    log::error!("{}", error);
                    return Err(error);
                }
            }
            Ok(events)
        })
        .await?)
    }

    pub async fn remove_requestor_events(&self, subscription_id: &SubscriptionId) -> DbResult<()> {
        let subscription_id = subscription_id.clone();
        do_with_transaction(self.pool, move |conn| {
            diesel::delete(dsl::market_event.filter(dsl::subscription_id.eq(&subscription_id)))
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}

impl<ErrorType: Into<DbError>> From<ErrorType> for TakeEventsError {
    fn from(err: ErrorType) -> Self {
        TakeEventsError::DatabaseError(err.into())
    }
}
