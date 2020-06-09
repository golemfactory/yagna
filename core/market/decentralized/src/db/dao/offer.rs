use chrono::Utc;
use thiserror::Error;

use ya_persistence::executor::Error as DbError;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

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
    #[error("Can't Unsubscribe not existing offer: {0}.")]
    OfferDoesntExist(SubscriptionId),
    #[error(transparent)]
    DatabaseError(#[from] DbError),
}

impl<'c> OfferDao<'c> {
    pub async fn get_offer(
        &self,
        subscription_id: &SubscriptionId,
    ) -> DbResult<Option<ModelOffer>> {
        let subscription_id = subscription_id.clone();
        let now = Utc::now().naive_utc();

        readonly_transaction(self.pool, move |conn| {
            let offer: Option<ModelOffer> = dsl::market_offer
                .filter(dsl::id.eq(&subscription_id))
                .filter(dsl::expiration_ts.ge(now))
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
        let unsubscribe = self
            .get_offer(&subscription_id)
            .await?
            .ok_or(UnsubscribeError::OfferDoesntExist(subscription_id.clone()))?
            .into_unsubscribe();

        Ok(do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl_unsubscribed::market_offer_unsubscribed)
                .values(unsubscribe)
                .execute(conn)?;
            DbResult::<()>::Ok(())
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
