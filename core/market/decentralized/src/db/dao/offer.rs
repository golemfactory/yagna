use chrono::Utc;
use thiserror::Error;

use ya_persistence::executor::Error as DbError;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

use crate::db::models::{NewOfferUnsubscribed, Offer as ModelOffer, OfferUnsubscribed};
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
    #[error("Can't unsubscribe not existing offer: {0}.")]
    OfferDoesntExist(SubscriptionId),
    #[error("Can't unsubscribe expired offer: {0}.")]
    OfferExpired(SubscriptionId),
    #[error("Offer [{0}] already unsubscribed.")]
    AlreadyUnsubscribed(SubscriptionId),
    #[error(transparent)]
    DatabaseError(DbError),
}

impl<'c> OfferDao<'c> {
    pub async fn get_offer(
        &self,
        subscription_id: &SubscriptionId,
    ) -> DbResult<Option<ModelOffer>> {
        let subscription_id = subscription_id.clone();
        let now = Utc::now().naive_utc();

        readonly_transaction(self.pool, move |conn| {
            if is_unsubscribed(conn, &subscription_id)? {
                return Ok(None);
            }

            let offer: Option<ModelOffer> = dsl::market_offer
                .filter(dsl::id.eq(&subscription_id))
                .filter(dsl::expiration_ts.gt(now))
                .first(conn)
                .optional()?;
            match offer {
                Some(model_offer) => Ok(Some(model_offer)),
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn create_offer(&self, offer: &ModelOffer) -> DbResult<()> {
        let mut offer = offer.clone();
        // Insertions timestamp should always reference our local time
        // of adding it to database, so we must reset it here.
        offer.insertion_ts = None;

        // TODO: We should check, if Offer with the same subscription id
        //  wasn't unsubscribed already.
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
            // If offer was already unsubscribed, we need to check it before we
            // will make other queries, because we will get at best integrity constraint error.
            if is_unsubscribed(conn, &subscription_id)? {
                Err(UnsubscribeError::AlreadyUnsubscribed(
                    subscription_id.clone(),
                ))?;
            }

            let offer: Option<ModelOffer> = dsl::market_offer
                .filter(dsl::id.eq(&subscription_id))
                .first(conn)
                .optional()?;

            let unsubscribe: NewOfferUnsubscribed = offer
                .ok_or(UnsubscribeError::OfferDoesntExist(subscription_id.clone()))
                .map(|offer| {
                    // Note: we don't unsubscribe expired Offers.
                    match offer.expiration_ts > Utc::now().naive_utc() {
                        true => Ok(offer),
                        false => Err(UnsubscribeError::OfferExpired(subscription_id.clone())),
                    }
                })??
                .into_unsubscribe();

            diesel::insert_into(dsl_unsubscribed::market_offer_unsubscribed)
                .values(unsubscribe)
                .execute(conn)?;
            Result::<(), UnsubscribeError>::Ok(())
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

pub fn is_unsubscribed(conn: &ConnType, subscription_id: &SubscriptionId) -> DbResult<bool> {
    let unsubscribed: Option<OfferUnsubscribed> = dsl_unsubscribed::market_offer_unsubscribed
        .filter(dsl_unsubscribed::id.eq(&subscription_id))
        .first(conn)
        .optional()?;
    Ok(unsubscribed.is_some())
}

impl<ErrorType: Into<DbError>> From<ErrorType> for UnsubscribeError {
    fn from(err: ErrorType) -> Self {
        UnsubscribeError::DatabaseError(err.into())
    }
}
