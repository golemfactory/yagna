use chrono::NaiveDateTime;

use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

use crate::db::models::{Offer, OfferUnsubscribed};
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

/// Internal Offer state.
///
/// Since we keep only Offers subscribed locally
/// (Offers from other nodes are removed upon unsubscribe)
/// Unsubscribed and Expired Offers are Options.
// TODO: cleanup external expired offers
#[derive(Clone)]
pub enum OfferState {
    Active(Offer),
    Unsubscribed(Option<Offer>),
    Expired(Option<Offer>),
    NotFound,
}

impl<'c> OfferDao<'c> {
    pub async fn select(
        &self,
        subscription_id: &SubscriptionId,
        validation_ts: NaiveDateTime,
    ) -> DbResult<OfferState> {
        let subscription_id = subscription_id.clone();
        readonly_transaction(self.pool, move |conn| {
            query_state(conn, &subscription_id, validation_ts)
        })
        .await
    }

    pub async fn get_offers(&self) -> DbResult<Vec<Offer>> {
        readonly_transaction(self.pool, move |conn| {
            Ok(dsl::market_offer.load::<Offer>(conn)?)
        })
        .await
    }

    pub async fn get_all_offers(&self) -> DbResult<Vec<Offer>> {
        readonly_transaction(self.pool, move |conn| {
            Ok(dsl::market_offer.load::<Offer>(conn)?)
        })
        .await
    }

    pub async fn insert(&self, offer: Offer) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_offer)
                .values(offer)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    /// In single transaction checks offer state and if `Active` inserts unsubscription marker.
    /// Returns state as before operation. `Active` means unsubscribe has succeeded.
    pub async fn mark_unsubscribed(
        &self,
        subscription_id: &SubscriptionId,
        validation_ts: NaiveDateTime,
    ) -> DbResult<OfferState> {
        let subscription_id = subscription_id.clone();
        do_with_transaction(self.pool, move |conn| {
            query_state(conn, &subscription_id, validation_ts).map(|state| match state {
                OfferState::Active(offer) => {
                    diesel::insert_into(dsl_unsubscribed::market_offer_unsubscribed)
                        .values(offer.clone().into_unsubscribe())
                        .execute(conn)
                        .map_err(From::from)
                        .map(|_| OfferState::Active(offer))
                }
                _ => Ok(state),
            })
        })
        .await?
    }

    pub async fn delete(&self, subscription_id: &SubscriptionId) -> DbResult<bool> {
        let subscription_id = subscription_id.clone();

        do_with_transaction(self.pool, move |conn| {
            let num_deleted = diesel::delete(dsl::market_offer.filter(dsl::id.eq(subscription_id)))
                .execute(conn)?;
            Ok(num_deleted > 0)
        })
        .await
    }
}

fn query_state(
    conn: &ConnType,
    subscription_id: &SubscriptionId,
    validation_ts: NaiveDateTime,
) -> DbResult<OfferState> {
    let offer: Option<Offer> = dsl::market_offer
        .filter(dsl::id.eq(&subscription_id))
        .first(conn)
        .optional()?;

    if is_unsubscribed(conn, subscription_id)? {
        return Ok(OfferState::Unsubscribed(offer));
    }

    Ok(match offer {
        None => OfferState::NotFound,
        Some(offer) => match offer.expiration_ts > validation_ts {
            true => OfferState::Active(offer),
            false => OfferState::Expired(Some(offer)),
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
