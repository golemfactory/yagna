use chrono::Utc;
use thiserror::Error;

use ya_persistence::executor::Error as DbError;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

use crate::db::models::{Offer as ModelOffer, OfferUnsubscribed};
use crate::db::schema::market_offer::dsl;
use crate::db::schema::market_offer_unsubscribed::dsl as dsl_unsubscribed;
use crate::db::DbResult;
use crate::SubscriptionId;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

pub struct OfferDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for OfferDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

#[derive(Error, Debug)]
pub enum UnsubscribeError {
    #[error("Can't unsubscribe not existing offer")]
    OfferNotFound,
    #[error("Can't unsubscribe expired offer")]
    OfferExpired,
    #[error("Offer already unsubscribed")]
    AlreadyUnsubscribed,
    #[error("Can't unsubscribe offer. Database error: {0}")]
    DatabaseError(DbError),
}

/// Internal Offer state.
///
/// Since we keep only Offers subscribed locally
/// (Offers from other nodes are removed upon unsubscribe)
/// Unsubscribed and Expired Offers are Options.
// TODO: cleanup external expired offers
pub enum OfferState {
    Active(ModelOffer),
    Unsubscribed(Option<ModelOffer>),
    Expired(Option<ModelOffer>),
    NotFound,
}

impl<'c> OfferDao<'c> {
    pub async fn get_offer(
        &self,
        subscription_id: &SubscriptionId,
    ) -> DbResult<Option<ModelOffer>> {
        let subscription_id = subscription_id.clone();
        let now = Utc::now().naive_utc();

        readonly_transaction(self.pool, move |conn| {
            match query_offer(conn, &subscription_id)? {
                OfferState::Active(model_offer) => Ok(Some(model_offer)),
                _ => Ok(None),
            }
        })
        .await
    }

    pub async fn get_offer_state(&self, subscription_id: &SubscriptionId) -> DbResult<OfferState> {
        let subscription_id = subscription_id.clone();
        readonly_transaction(self.pool, move |conn| query_offer(conn, &subscription_id)).await
    }

    pub async fn create_offer(&self, offer: &ModelOffer) -> DbResult<()> {
        let mut offer = offer.clone();
        // Insertions timestamp should always reference our local time
        // of adding it to database, so we must reset it here.
        offer.insertion_ts = None;

        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_offer)
                .values(offer)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn mark_offer_as_unsubscribed(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Result<(), UnsubscribeError> {
        let subscription_id = subscription_id.clone();
        Ok(do_with_transaction(self.pool, move |conn| {
            match query_offer(conn, &subscription_id)? {
                OfferState::Active(offer) => {
                    diesel::insert_into(dsl_unsubscribed::market_offer_unsubscribed)
                        .values(offer.into_unsubscribe())
                        .execute(conn)?;
                    Ok(())
                }
                OfferState::Expired(_) => Err(UnsubscribeError::OfferExpired),
                OfferState::Unsubscribed(_) => Err(UnsubscribeError::AlreadyUnsubscribed),
                OfferState::NotFound => Err(UnsubscribeError::OfferNotFound),
            }
        })
        .await?)
    }

    pub async fn remove_offer(&self, subscription_id: &SubscriptionId) -> DbResult<bool> {
        let subscription_id = subscription_id.clone();

        do_with_transaction(self.pool, move |conn| {
            let num_deleted = diesel::delete(dsl::market_offer.filter(dsl::id.eq(subscription_id)))
                .execute(conn)?;
            Ok(num_deleted > 0)
        })
        .await
    }
}

fn query_offer(conn: &ConnType, subscription_id: &SubscriptionId) -> DbResult<OfferState> {
    let offer: Option<ModelOffer> = dsl::market_offer
        .filter(dsl::id.eq(&subscription_id))
        .first(conn)
        .optional()?;

    if is_unsubscribed(conn, subscription_id)? {
        return Ok(OfferState::Unsubscribed(offer));
    }

    Ok(match offer {
        None => OfferState::NotFound,
        Some(offer) => match offer.expiration_ts > Utc::now().naive_utc() {
            true => OfferState::Active(offer),
            false => OfferState::Expired(Some(offer)), // TODO: cleanup external expired offers
        },
    })
}

fn is_unsubscribed(conn: &ConnType, subscription_id: &SubscriptionId) -> DbResult<bool> {
    Ok(dsl_unsubscribed::market_offer_unsubscribed
        .filter(dsl_unsubscribed::id.eq(&subscription_id))
        .first::<OfferUnsubscribed>(conn)
        .optional()?
        .is_some())
}

impl<ErrorType: Into<DbError>> From<ErrorType> for UnsubscribeError {
    fn from(e: ErrorType) -> Self {
        UnsubscribeError::DatabaseError(e.into())
    }
}
