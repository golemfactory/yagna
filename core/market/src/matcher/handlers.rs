//! Discovery protocol messages handlers
use futures::StreamExt;
use metrics::{counter, value};

use crate::db::model::{Offer, SubscriptionId};
use crate::matcher::error::ModifyOfferError;
use crate::protocol::discovery::{
    error::DiscoveryRemoteError,
    message::{OffersBcast, OffersRetrieved, RetrieveOffers, UnsubscribedOffersBcast},
};

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
    Ok(store
        .filter_out_known_offer_ids(msg.offer_ids)
        .await
        .map_err(|e| log::warn!("Error filtering Offers. Error: {}", e))?)
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
    Ok(added_offers_ids)
}

pub(super) async fn get_local_offers(
    store: SubscriptionStore,
    _caller: String,
    msg: RetrieveOffers,
) -> Result<Vec<Offer>, DiscoveryRemoteError> {
    value!(
        "market.offers.retrieved_by_remotes",
        msg.offer_ids.len() as u64
    );

    match store.get_offers(msg.offer_ids).await {
        Ok(offers) => Ok(offers),
        Err(e) => {
            log::error!("Failed to get batch offers. Error: {}", e);
            Err(DiscoveryRemoteError::InternalError(format!(
                "Failed to get offers from db."
            )))
        }
    }
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
                    .unsubscribe_offer(&offer_id, false, caller)
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
                            log::warn!(
                                "Failed to unsubscribe Offer [{}]. Error: {}",
                                &offer_id,
                                &e
                            );
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
