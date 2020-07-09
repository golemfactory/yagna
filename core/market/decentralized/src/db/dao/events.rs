use chrono::Utc;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use thiserror::Error;

use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};
use ya_persistence::executor::{ConnType, Error as DbError};

use crate::db::dao::demand::{demand_status, DemandState};
use crate::db::dao::offer::query_state;
use crate::db::dao::OfferState;
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
    pub async fn add_proposal_event(&self, proposal: Proposal, owner: OwnerType) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let event = MarketEvent::from_proposal(&proposal, owner);
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
        Ok(do_with_transaction(self.pool, move |conn| {
            // Check subscription wasn't unsubscribed or expired.
            validate_subscription(conn, &subscription_id, owner)?;

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

    pub async fn remove_events(&self, subscription_id: &SubscriptionId) -> DbResult<()> {
        let subscription_id = subscription_id.clone();
        do_with_transaction(self.pool, move |conn| {
            diesel::delete(dsl::market_event.filter(dsl::subscription_id.eq(&subscription_id)))
                .execute(conn)?;
            Ok(())
        })
        .await
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
        OwnerType::Provider => match query_state(conn, &subscription_id, Utc::now().naive_utc())? {
            OfferState::NotFound => Err(TakeEventsError::SubscriptionNotFound(
                subscription_id.clone(),
            ))?,
            OfferState::Expired(_) => Err(TakeEventsError::SubscriptionExpired(
                subscription_id.clone(),
            ))?,
            _ => Ok(()),
        },
    }
}

impl<ErrorType: Into<DbError>> From<ErrorType> for TakeEventsError {
    fn from(err: ErrorType) -> Self {
        TakeEventsError::DatabaseError(err.into())
    }
}
