//! Discovery protocol messages handlers
use futures::prelude::*;
use metrics::counter;

use crate::db::model::SubscriptionId;
use crate::matcher::error::ModifyOfferError;
use crate::protocol::discovery::message::{OffersBcast, OffersRetrieved, UnsubscribedOffersBcast};

use super::{resolver::Resolver, store::SubscriptionStore};

/// Returns only those of input offers ids, that were not yet known.
pub(super) async fn filter_out_known_offer_ids(
    store: SubscriptionStore,
    _caller: String,
    msg: OffersBcast,
) -> Result<Vec<SubscriptionId>, ()> {
    // We shouldn't propagate Offer, if we already have it in our database.
    // Note that when we broadcast our Offer, it will reach us too, so it concerns
    // not only Offers from other nodes.
    store
        .filter_out_known_offer_ids(msg.offer_ids)
        .await
        .map_err(|e| log::warn!("Error filtering Offers. Error: {}", e))
}

/// Returns only ids of those from input offers, that was successfully stored locally.
/// Also triggers Resolver to match newly stored Offers against local Demands.
pub(super) async fn receive_remote_offers(
    resolver: Resolver,
    caller: String,
    msg: OffersRetrieved,
) -> Result<Vec<SubscriptionId>, ()> {
    let added_offers_ids = futures::stream::iter(msg.offers.into_iter())
        .filter_map(|offer| {
            let resolver = resolver.clone();
            async move {
                resolver
                    .store
                    .save_offer(offer)
                    .await
                    .map(|offer| {
                        resolver.receive(&offer);
                        offer.id
                    })
                    .map_err(|e| log::info!("Skipping foreign Offer: {}", e))
                    .ok()
            }
        })
        .collect::<Vec<SubscriptionId>>()
        .await;

    counter!("market.offers.incoming", added_offers_ids.len() as u64);
    log::trace!(
        "Received {} new Offers from [{}]",
        added_offers_ids.len(),
        caller
    );

    if !added_offers_ids.is_empty() {
        resolver.store.notify();
    }
    Ok(added_offers_ids)
}

/// Returns only those of input offer ids, that were able to be unsubscribed locally.
pub(super) async fn receive_remote_offer_unsubscribes(
    store: SubscriptionStore,
    caller: String,
    msg: UnsubscribedOffersBcast,
) -> Result<Vec<SubscriptionId>, ()> {
    let new_unsubscribes = futures::stream::iter(msg.offer_ids.into_iter())
        .filter_map(|offer_id| {
            let store = store.clone();
            let caller = caller.parse().ok();
            async move {
                store
                    .unsubscribe_offer(&offer_id, caller)
                    .await
                    // Some errors don't mean we shouldn't propagate unsubscription.
                    .or_else(|e| match e {
                        ModifyOfferError::UnsubscribedNotRemoved(..) => Ok(()),
                        _ => Err(e),
                    })
                    // Collect Offers, that were correctly unsubscribed.
                    .map(|_| offer_id.clone())
                    .map_err(|e| match e {
                        // We don't want to warn about normal situations.
                        ModifyOfferError::AlreadyUnsubscribed(..)
                        | ModifyOfferError::Expired(..)
                        | ModifyOfferError::NotFound(..) => e,
                        _ => {
                            log::warn!("Failed to unsubscribe Offer [{offer_id}]. Error: {e}");
                            e
                        }
                    })
                    .ok()
            }
        })
        .collect::<Vec<SubscriptionId>>()
        .await;

    if !new_unsubscribes.is_empty() {
        counter!(
            "market.offers.unsubscribes.incoming",
            new_unsubscribes.len() as u64
        );
        log::trace!(
            "Received {} new Offers to unsubscribe from [{}]",
            new_unsubscribes.len(),
            caller,
        );
    }
    Ok(new_unsubscribes)
}
