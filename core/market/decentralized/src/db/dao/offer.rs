use chrono::NaiveDateTime;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use ya_client::model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

use crate::db::models::SubscriptionId;
use crate::db::models::{Offer, OfferUnsubscribed};
use crate::db::schema::market_offer::dsl;
use crate::db::schema::market_offer_unsubscribed::dsl as dsl_unsubscribed;
use crate::db::DbResult;

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
#[derive(Clone, derive_more::Display)]
pub enum OfferState {
    #[display(fmt = "Active")]
    Active(Offer),
    #[display(fmt = "Unsubscribed")]
    Unsubscribed(Option<Offer>),
    #[display(fmt = "Expired")]
    Expired(Option<Offer>),
    #[display(fmt = "NotFound")]
    NotFound,
}

impl<'c> OfferDao<'c> {
    pub async fn select(
        &self,
        id: &SubscriptionId,
        validation_ts: NaiveDateTime,
    ) -> DbResult<OfferState> {
        let id = id.clone();
        readonly_transaction(self.pool, move |conn| {
            query_state(conn, &id, &validation_ts)
        })
        .await
    }

    pub async fn get_offers(
        &self,
        node_id: Option<NodeId>,
        validation_ts: NaiveDateTime,
    ) -> DbResult<Vec<Offer>> {
        readonly_transaction(self.pool, move |conn| {
            let active_offers = dsl::market_offer
                .filter(dsl::id.ne_all(
                    dsl_unsubscribed::market_offer_unsubscribed.select(dsl_unsubscribed::id),
                ))
                .filter(dsl::expiration_ts.ge(validation_ts));
            Ok(match node_id {
                Some(ident) => active_offers
                    .filter(dsl::node_id.eq(ident))
                    .load::<Offer>(conn)?,
                _ => active_offers.load::<Offer>(conn)?,
            })
        })
        .await
    }

    pub async fn get_offers_before(
        &self,
        insertion_ts: NaiveDateTime,
        validation_ts: NaiveDateTime,
    ) -> DbResult<Vec<Offer>> {
        readonly_transaction(self.pool, move |conn| {
            Ok(dsl::market_offer
                // we querying less equal here and less then in Demands
                // not to duplicate pair subscribed at the very same moment
                // Demands are more privileged
                .filter(dsl::insertion_ts.le(insertion_ts))
                .filter(dsl::expiration_ts.ge(validation_ts))
                .filter(dsl::id.ne_all(
                    dsl_unsubscribed::market_offer_unsubscribed.select(dsl_unsubscribed::id),
                ))
                .order_by(dsl::creation_ts.asc())
                .load::<Offer>(conn)?)
        })
        .await
    }

    pub async fn insert(
        &self,
        offer: Offer,
        validation_ts: NaiveDateTime,
    ) -> DbResult<(bool, OfferState)> {
        if offer.expiration_ts < validation_ts {
            return Ok((false, OfferState::Expired(Some(offer))));
        }

        do_with_transaction(self.pool, move |conn| {
            let id = offer.id.clone();

            if is_unsubscribed(conn, &id)? {
                return Ok((false, OfferState::Unsubscribed(Some(offer))));
            }

            if let Some(offer) = query_offer(conn, &id)? {
                return Ok((false, is_expired(offer, &validation_ts)));
            };

            diesel::insert_into(dsl::market_offer)
                .values(offer)
                .execute(conn)?;
            // SQLite do does not support returning from insert,
            // so we need to query again to get insertion_ts
            let offer = query_offer(conn, &id)?.unwrap();
            Ok((true, OfferState::Active(offer)))
        })
        .await
    }

    /// In single transaction checks offer state and if `Active` inserts unsubscription marker.
    /// Returns state as before operation. `Active` means unsubscribe has succeeded.
    pub async fn mark_unsubscribed(
        &self,
        id: &SubscriptionId,
        validation_ts: NaiveDateTime,
    ) -> DbResult<OfferState> {
        let id = id.clone();
        do_with_transaction(self.pool, move |conn| {
            query_state(conn, &id, &validation_ts).map(|state| match state {
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

    pub async fn delete(&self, id: &SubscriptionId) -> DbResult<bool> {
        let id = id.clone();

        do_with_transaction(self.pool, move |conn| {
            let num_deleted =
                diesel::delete(dsl::market_offer.filter(dsl::id.eq(id))).execute(conn)?;
            Ok(num_deleted > 0)
        })
        .await
    }
}

pub(super) fn query_state(
    conn: &ConnType,
    id: &SubscriptionId,
    validation_ts: &NaiveDateTime,
) -> DbResult<OfferState> {
    let offer: Option<Offer> = query_offer(conn, &id)?;

    if is_unsubscribed(conn, id)? {
        return Ok(OfferState::Unsubscribed(offer));
    }

    Ok(match offer {
        None => OfferState::NotFound,
        Some(offer) => is_expired(offer, validation_ts),
    })
}

fn is_expired(offer: Offer, validation_ts: &NaiveDateTime) -> OfferState {
    match &offer.expiration_ts > validation_ts {
        true => OfferState::Active(offer),
        false => OfferState::Expired(Some(offer)),
    }
}

fn query_offer(conn: &ConnType, id: &SubscriptionId) -> DbResult<Option<Offer>> {
    Ok(dsl::market_offer
        .filter(dsl::id.eq(&id))
        .first(conn)
        .optional()?)
}

pub(super) fn is_unsubscribed(conn: &ConnType, id: &SubscriptionId) -> DbResult<bool> {
    Ok(dsl_unsubscribed::market_offer_unsubscribed
        .filter(dsl_unsubscribed::id.eq(&id))
        .first::<OfferUnsubscribed>(conn)
        .optional()?
        .is_some())
}
