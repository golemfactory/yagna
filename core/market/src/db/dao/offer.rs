use chrono::NaiveDateTime;
use diesel::expression::dsl::now as sql_now;
use diesel::sqlite::Sqlite;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use ya_client::model::NodeId;

use ya_persistence::executor::{do_with_transaction, readonly_transaction, ConnType, PoolType};

use crate::db::model::SubscriptionId;
use crate::db::model::{Offer, OfferUnsubscribed};
use crate::db::schema::market_offer::dsl as offer;
use crate::db::schema::market_offer::dsl::market_offer;
use crate::db::schema::market_offer_unsubscribed::dsl as unsubscribed;
use crate::db::schema::market_offer_unsubscribed::dsl::market_offer_unsubscribed;
use crate::db::{AsMixedDao, DbError, DbResult};

const QUERY_OFFERS_PAGE: usize = 150;

pub struct OfferDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsMixedDao<'a> for OfferDao<'a> {
    fn as_dao(_disk_pool: &'a PoolType, ram_pool: &'a PoolType) -> Self {
        Self { pool: ram_pool }
    }
}

/// Internal Offer state.
///
/// Unsubscribed and Expired Offers are Options
/// since we keep only Offers subscribed locally
/// (Offers from other nodes are removed upon unsubscribe).
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

type OfferSelect<'a, DB> =
    crate::db::schema::market_offer::BoxedQuery<'a, DB, crate::db::schema::market_offer::SqlType>;

fn active_market_offers<'a>(expiry_validation_ts: NaiveDateTime) -> OfferSelect<'a, Sqlite> {
    market_offer
        .filter(offer::expiration_ts.ge(expiry_validation_ts))
        .filter(
            offer::id.ne_all(
                market_offer_unsubscribed
                    .select(unsubscribed::id)
                    .filter(unsubscribed::expiration_ts.ge(expiry_validation_ts)),
            ),
        )
        .into_boxed()
}

impl OfferDao<'_> {
    /// Returns Offer state.
    pub async fn get_state(
        &self,
        id: &SubscriptionId,
        expiry_validation_ts: NaiveDateTime,
    ) -> DbResult<OfferState> {
        let id = id.clone();
        readonly_transaction(self.pool, "offer_dao_get_state", move |conn| {
            query_state(conn, &id, &expiry_validation_ts)
        })
        .await
    }

    /// Returns Offers for given criteria.
    pub async fn get_scan_offers(
        &self,
        inserted_after_ts: Option<NaiveDateTime>,
        expiry_validation_ts: NaiveDateTime,
        limit: Option<i64>,
    ) -> DbResult<Vec<Offer>> {
        readonly_transaction(self.pool, "get_scan_offers", move |conn| {
            let mut query =
                active_market_offers(expiry_validation_ts).order_by(offer::insertion_ts.asc());

            if let Some(limit) = limit {
                query = query.limit(limit);
            }
            if let Some(ts) = inserted_after_ts {
                query = query.filter(offer::insertion_ts.gt(ts))
            };

            Ok(query.load(conn)?)
        })
        .await
    }

    /// Returns Offers for given criteria.
    pub async fn get_offers(
        &self,
        ids: Option<Vec<SubscriptionId>>,
        node_ids: Option<Vec<NodeId>>,
        inserted_before_ts: Option<NaiveDateTime>,
        expiry_validation_ts: NaiveDateTime,
    ) -> DbResult<Vec<Offer>> {
        readonly_transaction(self.pool, "offer_dao_get_offers", move |conn| {
            let mut query =
                active_market_offers(expiry_validation_ts).order_by(offer::creation_ts.asc());

            if let Some(ids) = ids {
                query = query.filter(offer::id.eq_any(ids));
            };

            if let Some(ids) = node_ids {
                query = query.filter(offer::node_id.eq_any(ids));
            };

            if let Some(ts) = inserted_before_ts {
                query = query.filter(offer::insertion_ts.le(ts))
            };

            Ok(query.load(conn)?)
        })
        .await
    }

    pub async fn query_offers(
        &self,
        node_id: Option<NodeId>,
        after_insert_ts: Option<NaiveDateTime>,
        expiry_validation_ts: NaiveDateTime,
    ) -> DbResult<(Vec<SubscriptionId>, Option<NaiveDateTime>)> {
        readonly_transaction(self.pool, "offer_dao_query_offers", move |conn| {
            //let max_ts : Option<NaiveDateTime> = active_market_offers(expiry_validation_ts).select(offer::insertion_ts.max()).get_result(conn).optional()?;

            let mut query = active_market_offers(expiry_validation_ts);
            if let Some(after_insert_ts) = after_insert_ts {
                query = query.filter(offer::insertion_ts.gt(after_insert_ts));
            }
            if let Some(node_id) = node_id {
                query = query.filter(offer::node_id.eq(node_id));
            }

            let ids_and_ts: Vec<(SubscriptionId, Option<NaiveDateTime>)> = query
                .order_by(offer::insertion_ts)
                .limit(QUERY_OFFERS_PAGE as i64)
                .select((offer::id, offer::insertion_ts))
                .load(conn)?;
            let max_ts = if ids_and_ts.len() < QUERY_OFFERS_PAGE {
                None
            } else {
                ids_and_ts.iter().filter_map(|(_, ts)| *ts).max()
            };
            let ids = ids_and_ts.into_iter().map(|(id, _)| id).collect();

            Ok((ids, max_ts))
        })
        .await
    }

    /// Returns Offer ids for given `node_ids` or all.
    pub async fn get_offer_ids(
        &self,
        node_ids: Option<Vec<NodeId>>,
        expiry_validation_ts: NaiveDateTime,
    ) -> DbResult<Vec<SubscriptionId>> {
        readonly_transaction(self.pool, "offer_dao_get_offers_ids", move |conn| {
            let mut query = market_offer
                .select(offer::id)
                .filter(offer::expiration_ts.ge(expiry_validation_ts))
                .filter(
                    offer::id.ne_all(
                        market_offer_unsubscribed
                            .select(unsubscribed::id)
                            .filter(unsubscribed::expiration_ts.ge(expiry_validation_ts)),
                    ),
                )
                .order_by(offer::creation_ts.asc())
                .into_boxed();

            if let Some(ids) = node_ids {
                query = query.filter(offer::node_id.eq_any(ids));
            };

            Ok(query.load(conn)?)
        })
        .await
    }

    /// Returns Offer Unsubscription ids for given `node_ids` or all.
    pub async fn get_unsubscribed_ids(
        &self,
        node_ids: Option<Vec<NodeId>>,
        expiry_validation_ts: NaiveDateTime,
    ) -> DbResult<Vec<SubscriptionId>> {
        readonly_transaction(self.pool, "offer_dao_get_unsubscribed_ids", move |conn| {
            let mut query = market_offer_unsubscribed
                .select(unsubscribed::id)
                .filter(unsubscribed::expiration_ts.ge(expiry_validation_ts))
                .into_boxed();

            if let Some(ids) = node_ids {
                query = query.filter(unsubscribed::node_id.eq_any(ids));
            };

            Ok(query.load(conn)?)
        })
        .await
    }

    /// Returns only those from input Offer ids that are in `market_offer`
    /// or in `market_offer_unsubscribed` table.
    pub async fn get_known_ids(&self, ids: Vec<SubscriptionId>) -> DbResult<Vec<SubscriptionId>> {
        readonly_transaction(self.pool, "offer_dao_get_known_ids", move |conn| {
            let known_unsubscribed_ids = market_offer_unsubscribed
                .select(unsubscribed::id)
                .filter(unsubscribed::id.eq_any(&ids))
                .load::<SubscriptionId>(conn)?;

            // diesel does not support UNION operator
            let mut known_ids = market_offer
                .select(offer::id)
                .filter(offer::id.eq_any(&ids))
                .filter(offer::id.ne_all(&known_unsubscribed_ids))
                .load::<SubscriptionId>(conn)?;
            known_ids.extend(known_unsubscribed_ids);
            Ok(known_ids)
        })
        .await
    }

    /// Inserts Offer.
    /// Validates if it is not already expired or exists in DB.
    /// Returns pair `(false, offer_state)` if insert have not succeed,
    /// or `(true, Active(offer))` after successful insert.
    pub async fn put(
        &self,
        mut offer: Offer,
        expiry_validation_ts: NaiveDateTime,
    ) -> DbResult<(bool, OfferState)> {
        if offer.expiration_ts < expiry_validation_ts {
            return Ok((false, OfferState::Expired(Some(offer))));
        }

        do_with_transaction(self.pool, "offer_dao_put", move |conn| {
            let id = offer.id.clone();

            if is_unsubscribed(conn, &id)? {
                return Ok((false, OfferState::Unsubscribed(Some(offer))));
            }

            if let Some(offer) = query_offer(conn, &id)? {
                return Ok((false, active_or_expired(offer, &expiry_validation_ts)));
            };

            // We need more precise timestamps, than auto-generated by db.
            // We must set them under transaction to avoid giving so timestamps
            // will be assigned in order of insertions to database.
            offer.insertion_ts = Some(chrono::Utc::now().naive_utc());

            diesel::insert_into(market_offer)
                .values(offer)
                .execute(conn)?;
            // SQLite do does not support returning from insert,
            // so we need to query again to get insertion_ts
            let offer = query_offer(conn, &id)?.unwrap();
            Ok((true, OfferState::Active(offer)))
        })
        .await
    }

    /// Inserts Offer unsubscription marker.
    /// Returns Offer state as before operation
    /// (`Active` means unsubscription has succeeded).
    pub async fn unsubscribe(
        &self,
        id: &SubscriptionId,
        expiry_validation_ts: NaiveDateTime,
    ) -> DbResult<OfferState> {
        let id = id.clone();
        do_with_transaction(self.pool, "offer_dao_unsubscribe", move |conn| {
            query_state(conn, &id, &expiry_validation_ts).map(|state| match state {
                OfferState::Active(offer) => diesel::insert_into(market_offer_unsubscribed)
                    .values(offer.clone().into_unsubscribe())
                    .execute(conn)
                    .map_err(From::from)
                    .map(|_| OfferState::Active(offer)),
                _ => Ok(state),
            })
        })
        .await?
    }

    /// Deletes single Offer.
    /// Returns `true` on success.
    pub async fn delete(&self, id: &SubscriptionId) -> DbResult<bool> {
        let id = id.clone();

        do_with_transaction(self.pool, "offer_dao_delete", move |conn| {
            let num_deleted =
                diesel::delete(market_offer.filter(offer::id.eq(id))).execute(conn)?;
            Ok(num_deleted > 0)
        })
        .await
    }

    pub async fn clean(&self) -> DbResult<()> {
        log::debug!("Clean market offers: start");
        let num_deleted = do_with_transaction(self.pool, "offer_dao_clean", move |conn| {
            let nd = diesel::delete(market_offer.filter(offer::expiration_ts.lt(sql_now)))
                .execute(conn)?;
            Result::<usize, DbError>::Ok(nd)
        })
        .await?;
        if num_deleted > 0 {
            log::info!("Clean market offers: {} cleaned", num_deleted);
        }
        self.clean_unsubscribes().await?;
        log::debug!("Clean market offers: done");
        Ok(())
    }

    pub async fn clean_unsubscribes(&self) -> DbResult<()> {
        log::debug!("Clean market offers unsubscribes: start");
        let num_deleted =
            do_with_transaction(self.pool, "offer_dao_clean_unsubscribes", move |conn| {
                let nd = diesel::delete(
                    market_offer_unsubscribed.filter(unsubscribed::expiration_ts.lt(sql_now)),
                )
                .execute(conn)?;
                Result::<usize, DbError>::Ok(nd)
            })
            .await?;
        if num_deleted > 0 {
            log::info!("Clean market offers unsubscribes: {} cleaned", num_deleted);
        }
        log::debug!("Clean market offers unsubscribes: done");
        Ok(())
    }
}

pub(super) fn query_state(
    conn: &ConnType,
    id: &SubscriptionId,
    expiry_validation_ts: &NaiveDateTime,
) -> DbResult<OfferState> {
    let offer: Option<Offer> = query_offer(conn, id)?;

    if is_unsubscribed(conn, id)? {
        return Ok(OfferState::Unsubscribed(offer));
    }

    Ok(match offer {
        None => OfferState::NotFound,
        Some(offer) => active_or_expired(offer, expiry_validation_ts),
    })
}

fn active_or_expired(offer: Offer, expiry_validation_ts: &NaiveDateTime) -> OfferState {
    match &offer.expiration_ts > expiry_validation_ts {
        true => OfferState::Active(offer),
        false => OfferState::Expired(Some(offer)),
    }
}

fn query_offer(conn: &ConnType, id: &SubscriptionId) -> DbResult<Option<Offer>> {
    Ok(market_offer
        .filter(offer::id.eq(&id))
        .first(conn)
        .optional()?)
}

pub(super) fn is_unsubscribed(conn: &ConnType, id: &SubscriptionId) -> DbResult<bool> {
    Ok(market_offer_unsubscribed
        .filter(unsubscribed::id.eq(&id))
        .first::<OfferUnsubscribed>(conn)
        .optional()?
        .is_some())
}
