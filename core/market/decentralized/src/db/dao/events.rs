use chrono::Utc;
use thiserror::Error;

use ya_persistence::executor::Error as DbError;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

use crate::db::models::MarketEvent;
use crate::db::models::Offer as ModelOffer;
use crate::db::models::{Demand as ModelDemand, SubscriptionId};
use crate::db::models::{Negotiation, OwnerType, Proposal, ProposalExt};
use crate::db::schema::market_provider_event::dsl as dsl_provider;
use crate::db::schema::market_requestor_event::dsl as dsl_requestor;
use crate::db::DbResult;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

#[derive(Error, Debug)]
pub enum TakeEventsError {
    #[error("Removed different number of events '{num_removed}' that expected '{to_remove}'.")]
    EventsRemovalError {
        num_removed: usize,
        to_remove: usize,
    },
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
    pub async fn add_requestor_event(
        &self,
        proposal: Proposal,
        negotiation: Negotiation,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let event = MarketEvent::from_proposal(&proposal, &negotiation, OwnerType::Requestor);
            diesel::insert_into(dsl_requestor::market_requestor_event)
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
            let events = dsl_requestor::market_requestor_event
                .filter(dsl_requestor::subscription_id.eq(&subscription_id))
                .order_by(dsl_requestor::timestamp.asc())
                .limit(max_events as i64)
                .load::<MarketEvent>(conn)?;

            // Remove events from queue.
            // We check if events are older, than first events that is the newest
            // since we ordered them ascending.
            // Is it safe?? Can we remove by accident events, that shouldn't be removed?
            let first_event = events.first();
            if let Some(first_event) = first_event {
                let num_removed = diesel::delete(
                    dsl_requestor::market_requestor_event
                        .filter(dsl_requestor::subscription_id.eq(&subscription_id))
                        .filter(dsl_requestor::timestamp.le(first_event.timestamp)),
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
}

impl<ErrorType: Into<DbError>> From<ErrorType> for TakeEventsError {
    fn from(err: ErrorType) -> Self {
        TakeEventsError::DatabaseError(err.into())
    }
}
