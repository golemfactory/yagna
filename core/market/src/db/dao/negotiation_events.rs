use chrono::Utc;
use diesel::dsl::sql;
use diesel::{sql_types, ExpressionMethods, QueryDsl, RunQueryDsl};
use thiserror::Error;

use ya_client::model::market::Reason;
use ya_persistence::executor::ConnType;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::db::dao::demand::{demand_status, DemandState};
use crate::db::dao::offer::{query_state, OfferState};
use crate::db::dao::sql_functions::datetime;
use crate::db::model::{Agreement, EventType, MarketEvent, Owner, Proposal, SubscriptionId};
use crate::db::schema::market_negotiation_event::dsl;
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
    NotFound(SubscriptionId),
    #[error("Subscription [{0}] expired.")]
    Expired(SubscriptionId),
    #[error("Failed to get events from DB: {0}.")]
    Db(DbError),
}

pub struct NegotiationEventsDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for NegotiationEventsDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> NegotiationEventsDao<'c> {
    pub async fn add_proposal_event(&self, proposal: &Proposal, role: Owner) -> DbResult<()> {
        let event = MarketEvent::from_proposal(proposal, role);
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_negotiation_event)
                .values(event)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn add_proposal_rejected_event(
        &self,
        proposal: &Proposal,
        reason: Option<Reason>,
    ) -> DbResult<()> {
        let event = MarketEvent::proposal_rejected(proposal, reason);
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_negotiation_event)
                .values(event)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn add_agreement_event(&self, agreement: &Agreement) -> DbResult<()> {
        let event = MarketEvent::from_agreement(agreement);
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_negotiation_event)
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
        owner: Owner,
    ) -> Result<Vec<MarketEvent>, TakeEventsError> {
        let subscription_id = subscription_id.clone();
        do_with_transaction(self.pool, move |conn| {
            // Check subscription wasn't unsubscribed or expired.
            validate_subscription(conn, &subscription_id, owner)?;

            // Only ProposalEvents should be in random order.
            //  AgreementEvent and rejections events should be sorted with higher
            //  priority.
            let basic_query =
                dsl::market_negotiation_event.filter(dsl::subscription_id.eq(&subscription_id));
            let mut events = basic_query
                .clone()
                .filter(dsl::event_type.ne_all(vec![
                    EventType::ProviderNewProposal,
                    EventType::RequestorNewProposal,
                ]))
                .order_by(dsl::timestamp.asc())
                .limit(max_events as i64)
                .load::<MarketEvent>(conn)?;
            if (events.len() as i32) < max_events {
                let limit_left: i32 = max_events - (events.len() as i32);
                let proposal_events = basic_query
                    .filter(dsl::event_type.eq_any(vec![
                        EventType::ProviderNewProposal,
                        EventType::RequestorNewProposal,
                    ]))
                    .order_by(sql::<sql_types::Bool>("RANDOM()"))
                    .limit(limit_left as i64)
                    .load::<MarketEvent>(conn)?;

                events.extend(proposal_events.into_iter());
            }

            // Remove returned events from queue.
            if !events.is_empty() {
                let ids = events.iter().map(|event| event.id).collect::<Vec<_>>();
                diesel::delete(dsl::market_negotiation_event.filter(dsl::id.eq_any(ids)))
                    .execute(conn)?;
            }

            Ok(events)
        })
        .await
    }

    pub async fn remove_events(&self, subscription_id: &SubscriptionId) -> DbResult<()> {
        let subscription_id = subscription_id.clone();
        do_with_transaction(self.pool, move |conn| {
            diesel::delete(
                dsl::market_negotiation_event.filter(dsl::subscription_id.eq(&subscription_id)),
            )
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
                dsl::market_negotiation_event
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
    owner: Owner,
) -> Result<(), TakeEventsError> {
    match owner {
        Owner::Requestor => match demand_status(conn, &subscription_id)? {
            DemandState::NotFound => Err(TakeEventsError::NotFound(subscription_id.clone()))?,
            DemandState::Expired(_) => Err(TakeEventsError::Expired(subscription_id.clone()))?,
            _ => Ok(()),
        },
        Owner::Provider => match query_state(conn, &subscription_id, &Utc::now().naive_utc())? {
            OfferState::NotFound => Err(TakeEventsError::NotFound(subscription_id.clone()))?,
            OfferState::Expired(_) => Err(TakeEventsError::Expired(subscription_id.clone()))?,
            _ => Ok(()),
        },
    }
}

impl<ErrorType: Into<DbError>> From<ErrorType> for TakeEventsError {
    fn from(err: ErrorType) -> Self {
        TakeEventsError::Db(err.into())
    }
}
