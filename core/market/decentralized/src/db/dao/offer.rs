use ya_persistence::executor::Error;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::db::models::Offer as ModelOffer;
use crate::db::schema::market_offer::dsl;
use crate::db::DbResult;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

#[allow(unused)]
pub struct OfferDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for OfferDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> OfferDao<'c> {
    pub async fn get_offer<Str: AsRef<str>>(
        &self,
        subscription_id: Str,
    ) -> DbResult<Option<ModelOffer>> {
        let subscription_id = subscription_id.as_ref().to_string();
        readonly_transaction(self.pool, move |conn| {
            let offer: Option<ModelOffer> = dsl::market_offer
                .filter(dsl::id.eq(&subscription_id))
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
        let offer = offer.clone();
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_offer)
                .values(offer)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn remove_offer<Str: AsRef<str>>(&self, subscription_id: Str) -> DbResult<bool> {
        let subscription_id = subscription_id.as_ref().to_string();

        do_with_transaction(self.pool, move |conn| {
            let num_deleted = diesel::delete(dsl::market_offer.filter(dsl::id.eq(subscription_id)))
                .execute(conn)?;
            Ok(num_deleted > 0)
        })
        .await
    }
}
