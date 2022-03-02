//! Discovery protocol interface
use actix_rt::Arbiter;
use metrics::{counter, timing, value};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::delay_for;

use ya_client::model::NodeId;
use ya_core_model::market::BUS_ID;
use ya_core_model::net::local::{BroadcastMessage, SendBroadcastMessage};
use ya_net::{self as net, RemoteEndpoint};
use ya_service_bus::timeout::{IntoDuration, IntoTimeoutFuture};
use ya_service_bus::typed::ServiceBinder;
use ya_service_bus::{Error as BusError, RpcEndpoint, RpcMessage};

use super::callback::HandlerSlot;
use crate::config::DiscoveryConfig;
use crate::db::model::{Offer as ModelOffer, SubscriptionId};
use crate::identity::{IdentityApi, IdentityError};

pub mod builder;
pub mod error;
pub mod message;

use crate::PROTOCOL_VERSION;
use error::*;
use futures::TryFutureExt;
use message::*;

/// Responsible for communication with markets on other nodes
/// during discovery phase.
#[derive(Clone)]
pub struct Discovery {
    inner: Arc<DiscoveryImpl>,
}

pub(super) struct OfferHandlers {
    filter_out_known_ids: HandlerSlot<OffersBcast>,
    receive_remote_offers: HandlerSlot<OffersRetrieved>,
}

pub struct DiscoveryImpl {
    identity: Arc<dyn IdentityApi>,

    offer_queue: Mutex<Vec<SubscriptionId>>,
    unsub_queue: Mutex<Vec<SubscriptionId>>,
    lazy_binder_prefix: Mutex<Option<String>>,

    offer_handlers: Mutex<OfferHandlers>,
    get_local_offers_handler: HandlerSlot<RetrieveOffers>,
    offer_unsubscribe_handler: HandlerSlot<UnsubscribedOffersBcast>,

    config: DiscoveryConfig,
}

impl Discovery {
    pub fn re_broadcast_enabled(&self) -> bool {
        match std::env::var("YA_NET_TYPE") {
            Ok(val) => val == "hybrid",
            Err(_) => false,
        }
    }

    pub async fn bcast_offers(&self, offer_ids: Vec<SubscriptionId>) -> Result<(), DiscoveryError> {
        if offer_ids.is_empty() {
            return Ok(());
        }
        // When there are 0 items in the queue we should schedule a send job.
        let must_schedule = {
            let mut queue = self.inner.offer_queue.lock().await;
            let result = queue.len() == 0;

            queue.append(&mut offer_ids.clone());
            result
        };
        log::trace!(
            "bcast_offers done appending {} offers. must_schedule={}",
            offer_ids.len(),
            must_schedule
        );

        if must_schedule {
            let myself = self.clone();
            let _ = Arbiter::spawn(async move {
                // Sleep to collect multiple offers to send
                delay_for(myself.inner.config.offer_broadcast_delay).await;
                myself.send_bcast_offers().await;
            });
        }
        Ok(())
    }

    /// Broadcasts Offers to other nodes in network. Connected nodes will
    /// get call to function bound at `OfferBcast`.
    async fn send_bcast_offers(&self) -> () {
        // `...offer_queue` MUST be empty to trigger the sending again
        let offer_ids: Vec<SubscriptionId> =
            self.inner.offer_queue.lock().await.drain(..).collect();

        // Should never happen, but just to be certain.
        if offer_ids.is_empty() {
            return ();
        }

        let default_id = match self.default_identity().await {
            Ok(id) => id,
            Err(e) => {
                log::error!(
                    "Error getting default identity, not sending bcast. error={:?}",
                    e
                );
                return;
            }
        };
        let size = offer_ids.len();
        log::debug!("Broadcasting offers. count={}", size);

        counter!("market.offers.broadcasts.net", 1);
        value!("market.offers.broadcasts.len", size as u64);

        // TODO: should we send as our (default) identity?
        if let Err(e) = net::broadcast(default_id, OffersBcast { offer_ids }).await {
            log::error!("Error sending bcast, skipping... error={:?}", e);
            counter!("market.offers.broadcasts.net_errors", 1);
        };
    }

    /// Ask remote Node for specified Offers.
    pub async fn get_remote_offers(
        &self,
        target_node_id: String,
        offer_ids: Vec<SubscriptionId>,
        timeout: impl IntoDuration,
    ) -> Result<Vec<ModelOffer>, DiscoveryError> {
        let target_node = NodeId::from_str(&target_node_id)
            .map_err(|e| DiscoveryError::InternalError(e.to_string()))?;

        Ok(net::from(self.default_identity().await?)
            .to(target_node)
            .service(&get_offers_addr(BUS_ID))
            .send(RetrieveOffers { offer_ids })
            .timeout(Some(timeout))
            .map_err(|_| {
                DiscoveryError::GsbError(
                    BusError::Timeout(format!(
                        "{}/{}",
                        get_offers_addr(BUS_ID),
                        RetrieveOffers::ID
                    ))
                    .to_string(),
                )
            })
            .await???)
    }

    pub async fn bcast_unsubscribes(
        &self,
        offer_ids: Vec<SubscriptionId>,
    ) -> Result<(), DiscoveryError> {
        if offer_ids.is_empty() {
            return Ok(());
        }

        // When there are 0 items in the queue we should schedule a send job.
        let must_schedule = {
            let mut queue = self.inner.unsub_queue.lock().await;
            let result = queue.len() == 0;

            queue.append(&mut offer_ids.clone());
            result
        };

        log::trace!(
            "bcast_unsubscribes done appending {} offers. must_schedule={}",
            offer_ids.len(),
            must_schedule
        );

        if must_schedule {
            let myself = self.clone();
            let _ = Arbiter::spawn(async move {
                // Sleep to collect multiple unsubscribes to send
                delay_for(myself.inner.config.unsub_broadcast_delay).await;
                myself.send_bcast_unsubscribes().await;
            });
        }
        Ok(())
    }

    async fn send_bcast_unsubscribes(&self) {
        // `...unsub_queue` MUST be empty to trigger the sending again
        let offer_ids: Vec<SubscriptionId> =
            self.inner.unsub_queue.lock().await.drain(..).collect();

        // Should never happen, but just to be certain.
        if offer_ids.is_empty() {
            return ();
        }
        let default_id = match self.default_identity().await {
            Ok(id) => id,
            Err(e) => {
                log::error!(
                    "Error getting default identity, not sending bcast. error={:?}",
                    e
                );
                return;
            }
        };

        let size = offer_ids.len();
        log::debug!("Broadcasting unsubscribes. count={}", size);
        counter!("market.offers.unsubscribes.broadcasts.net", 1);
        value!("market.offers.unsubscribes.broadcasts.len", size as u64);

        // TODO: should we send as our (default) identity?
        if let Err(e) = net::broadcast(default_id, UnsubscribedOffersBcast { offer_ids }).await {
            log::error!("Error sending bcast, skipping... error={:?}", e);
            counter!("market.offers.unsubscribes.broadcasts.net_errors", 1);
        };
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        local_prefix: &str,
    ) -> Result<(), DiscoveryInitError> {
        log::info!("Discovery protocol version: {}", PROTOCOL_VERSION!());

        ServiceBinder::new(&get_offers_addr(public_prefix), &(), self.clone()).bind_with_processor(
            move |_, myself, caller: String, msg: RetrieveOffers| {
                let myself = myself.clone();
                myself.on_get_remote_offers(caller, msg)
            },
        );
        // Subscribe to offer broadcasts.
        {
            let mut prefix_guard = self.inner.lazy_binder_prefix.lock().await;
            if let Some(old_prefix) = (*prefix_guard).replace(local_prefix.to_string()) {
                log::info!("Dropping previous lazy_binder_prefix, and replacing it with new one. old={}, new={}", old_prefix, local_prefix);
            };
        }

        // Only bind broadcasts when re-broadcasts are enabled
        if self.re_broadcast_enabled() {
            // We don't lazy bind broadcasts handlers anymore on first Demand creation.
            // But we still have option to do this easily in the future.
            self.bind_gsb_broadcast().await.map_or_else(
                |e| {
                    log::warn!("Failed to subscribe to broadcasts. Error: {:?}.", e,);
                },
                |_| (),
            );
        }

        Ok(())
    }

    pub async fn bind_gsb_broadcast(&self) -> Result<(), DiscoveryInitError> {
        log::trace!("GsbBroadcastBind");
        let myself = self.clone();

        // /local/market/market-protocol-mk1-offer
        let mut prefix_guard = self.inner.lazy_binder_prefix.lock().await;
        let local_prefix = match (*prefix_guard).take() {
            None => return Ok(()),
            Some(prefix) => prefix,
        };

        let bcast_address = format!("{}/{}", local_prefix.as_str(), OffersBcast::TOPIC);
        ya_net::bind_broadcast_with_caller(
            &bcast_address,
            move |caller, msg: SendBroadcastMessage<OffersBcast>| {
                let myself = myself.clone();
                myself.on_bcast_offers(caller, msg.body().to_owned())
            },
        )
        .await
        .map_err(|e| DiscoveryInitError::from_pair(bcast_address, e))?;

        let myself = self.clone();
        // /local/market/market-protocol-mk1-offer-unsubscribe
        let bcast_address = format!("{}/{}", local_prefix, UnsubscribedOffersBcast::TOPIC);
        ya_net::bind_broadcast_with_caller(
            &bcast_address,
            move |caller, msg: SendBroadcastMessage<UnsubscribedOffersBcast>| {
                let myself = myself.clone();
                myself.on_bcast_unsubscribes(caller, msg.body().to_owned())
            },
        )
        .await
        .map_err(|e| DiscoveryInitError::from_pair(bcast_address, e))?;
        Ok(())
    }

    async fn on_bcast_offers(self, caller: String, msg: OffersBcast) -> Result<(), ()> {
        let start = Instant::now();
        let num_ids_received = msg.offer_ids.len();
        log::trace!("Received {} Offers from [{}].", num_ids_received, &caller);
        if msg.offer_ids.is_empty() {
            return Ok(());
        }

        // We should do filtering and getting Offers in single transaction. Otherwise multiple
        // broadcasts can overlap and we will ask other nodes for the same Offers more than once.
        // Note that it wouldn't cause incorrect behavior, because we will add Offers only once.
        // Other attempts to add them will end with error and we will filter all Offers, that already
        // occurred and re-broadcast only new ones.
        // But still it is worth to limit network traffic.
        let new_offer_ids = {
            let offer_handlers = match self.inner.offer_handlers.try_lock() {
                Ok(h) => h,
                Err(_) => {
                    log::trace!("Already handling bcast_offers, skipping...");
                    counter!("market.offers.broadcasts.skip", 1);
                    return Ok(());
                }
            };
            let filter_out_known_ids = offer_handlers.filter_out_known_ids.clone();
            let receive_remote_offers = offer_handlers.receive_remote_offers.clone();

            let unknown_offer_ids = filter_out_known_ids.call(caller.clone(), msg).await?;

            if !unknown_offer_ids.is_empty() {
                let start_remote = Instant::now();
                let offers = self
                    .get_remote_offers(caller.clone(), unknown_offer_ids, 3)
                    .await
                    .map_err(|e| {
                        log::debug!("Can't get Offers from [{}]. Error: {}", &caller, e)
                    })?;
                let end_remote = Instant::now();
                timing!(
                    "market.offers.incoming.get_remote.time",
                    start_remote,
                    end_remote
                );

                // We still could fail to add some Offers to database. If we fail to add them, we don't
                // want to propagate subscription further.
                receive_remote_offers
                    .call(caller.clone(), OffersRetrieved { offers })
                    .await?
            } else {
                vec![]
            }
        };

        if self.re_broadcast_enabled() && !new_offer_ids.is_empty() {
            log::trace!(
                "Propagating {}/{} Offers received from [{}].",
                new_offer_ids.len(),
                num_ids_received,
                &caller
            );

            // We could broadcast outside of lock, but it shouldn't hurt either, because
            // we don't wait for any responses from remote nodes.
            self.bcast_offers(new_offer_ids)
                .await
                .map_err(|e| log::warn!("Failed to bcast. Error: {}", e))?;
        }

        let end = Instant::now();
        timing!("market.offers.incoming.time", start, end);
        Ok(())
    }

    async fn on_get_remote_offers(
        self,
        caller: String,
        msg: RetrieveOffers,
    ) -> Result<Vec<ModelOffer>, DiscoveryRemoteError> {
        log::trace!("[{}] asks for {} Offers.", &caller, msg.offer_ids.len());
        let get_local_offers = self.inner.get_local_offers_handler.clone();
        Ok(get_local_offers.call(caller, msg).await?)
    }

    async fn on_bcast_unsubscribes(
        self,
        caller: String,
        msg: UnsubscribedOffersBcast,
    ) -> Result<(), ()> {
        let start = Instant::now();
        let num_received_ids = msg.offer_ids.len();
        log::trace!(
            "Received {} unsubscribed Offers from [{}].",
            num_received_ids,
            &caller
        );
        if msg.offer_ids.is_empty() {
            return Ok(());
        }

        let offer_unsubscribe_handler = self.inner.offer_unsubscribe_handler.clone();
        let unsubscribed_offer_ids = offer_unsubscribe_handler.call(caller.clone(), msg).await?;

        if self.re_broadcast_enabled() && !unsubscribed_offer_ids.is_empty() {
            log::trace!(
                "Propagating {}/{} unsubscribed Offers received from [{}].",
                unsubscribed_offer_ids.len(),
                num_received_ids,
                &caller,
            );

            // No need to retry broadcasting, since we send cyclic broadcasts.
            if let Err(error) = self.bcast_unsubscribes(unsubscribed_offer_ids).await {
                log::error!("Error propagating unsubscribed Offers further: {}", error,);
            }
        }
        let end = Instant::now();
        timing!("market.offers.unsubscribes.incoming.time", start, end);
        Ok(())
    }

    async fn default_identity(&self) -> Result<NodeId, IdentityError> {
        Ok(self.inner.identity.default_identity().await?)
    }
}
