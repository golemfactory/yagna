//! Discovery protocol interface
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use ya_client::model::NodeId;
use ya_core_model::market::BUS_ID;
use ya_core_model::net::local as local_net;
use ya_core_model::net::local::{BroadcastMessage, SendBroadcastMessage};
use ya_net::{self as net, RemoteEndpoint};
use ya_service_bus::typed::ServiceBinder;
use ya_service_bus::{typed as bus, RpcEndpoint};

use super::callback::HandlerSlot;
use crate::db::model::{Offer as ModelOffer, SubscriptionId};
use crate::identity::{IdentityApi, IdentityError};

pub mod builder;
pub mod error;
pub mod message;

use crate::PROTOCOL_VERSION;
use error::*;
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

    offer_handlers: Mutex<OfferHandlers>,
    get_local_offers_handler: HandlerSlot<RetrieveOffers>,
    offer_unsubscribe_handler: HandlerSlot<UnsubscribedOffersBcast>,
}

impl Discovery {
    /// Broadcasts Offers to other nodes in network. Connected nodes will
    /// get call to function bound at `OfferBcast`.
    pub async fn bcast_offers(&self, offer_ids: Vec<SubscriptionId>) -> Result<(), DiscoveryError> {
        let default_id = self.default_identity().await?;
        let bcast_msg = SendBroadcastMessage::new(OffersBcast { offer_ids });

        // TODO: We shouldn't use send_as. Put identity inside broadcasted message instead.
        let _ = bus::service(local_net::BUS_ID)
            .send_as(default_id, bcast_msg) // TODO: should we send as our (default) identity?
            .await?;
        Ok(())
    }

    /// Ask remote Node for specified Offers.
    pub async fn get_remote_offers(
        &self,
        target_node_id: String,
        offer_ids: Vec<SubscriptionId>,
    ) -> Result<Vec<ModelOffer>, DiscoveryError> {
        let target_node = NodeId::from_str(&target_node_id)
            .map_err(|e| DiscoveryError::InternalError(e.to_string()))?;

        Ok(net::from(self.default_identity().await?)
            .to(target_node)
            .service(&get_offers_addr(BUS_ID))
            .send(RetrieveOffers { offer_ids })
            .await??)
    }

    pub async fn bcast_unsubscribes(
        &self,
        offer_ids: Vec<SubscriptionId>,
    ) -> Result<(), DiscoveryError> {
        let default_id = self.default_identity().await?;

        let bcast_msg = SendBroadcastMessage::new(UnsubscribedOffersBcast { offer_ids });

        // TODO: We shouldn't use send_as. Put identity inside broadcasted message instead.
        let _ = bus::service(local_net::BUS_ID)
            .send_as(default_id, bcast_msg)
            .await?;
        Ok(())
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        local_prefix: &str,
    ) -> Result<(), DiscoveryInitError> {
        log::info!("Discovery protocol version: {}", PROTOCOL_VERSION!());

        let myself = self.clone();
        // /local/market/market-protocol-mk1-offer
        let bcast_address = format!("{}/{}", local_prefix, OffersBcast::TOPIC);
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

        ServiceBinder::new(&get_offers_addr(public_prefix), &(), self.clone()).bind_with_processor(
            move |_, myself, caller: String, msg: RetrieveOffers| {
                let myself = myself.clone();
                myself.on_get_remote_offers(caller, msg)
            },
        );

        Ok(())
    }

    async fn on_bcast_offers(self, caller: String, msg: OffersBcast) -> Result<(), ()> {
        let num_ids_received = msg.offer_ids.len();
        if !msg.offer_ids.is_empty() {
            log::debug!("Received {} Offers from [{}].", num_ids_received, &caller);
        }

        // We should do filtering and getting Offers in single transaction. Otherwise multiple
        // broadcasts can overlap and we will ask other nodes for the same Offers more than once.
        // Note that it wouldn't cause incorrect behavior, because we will add Offers only once.
        // Other attempts to add them will end with error and we will filter all Offers, that already
        // occurred and re-broadcast only new ones.
        // But still it is worth to limit network traffic.
        let new_offer_ids = {
            let offer_handlers = self.inner.offer_handlers.lock().await;
            let filter_out_known_ids = offer_handlers.filter_out_known_ids.clone();
            let receive_remote_offers = offer_handlers.receive_remote_offers.clone();

            let unknown_offer_ids = filter_out_known_ids.call(caller.clone(), msg).await?;

            if !unknown_offer_ids.is_empty() {
                let offers = self
                    .get_remote_offers(caller.clone(), unknown_offer_ids)
                    .await
                    .map_err(|e| {
                        log::debug!("Can't get Offers from [{}]. Error: {}", &caller, e)
                    })?;

                // We still could fail to add some Offers to database. If we fail to add them, we don't
                // want to propagate subscription further.
                receive_remote_offers
                    .call(caller.clone(), OffersRetrieved { offers })
                    .await?
            } else {
                vec![]
            }
        };

        if !new_offer_ids.is_empty() {
            log::debug!(
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

        Ok(())
    }

    async fn on_get_remote_offers(
        self,
        caller: String,
        msg: RetrieveOffers,
    ) -> Result<Vec<ModelOffer>, DiscoveryRemoteError> {
        log::debug!("[{}] asks for {} Offers.", &caller, msg.offer_ids.len());
        let get_local_offers = self.inner.get_local_offers_handler.clone();
        Ok(get_local_offers.call(caller, msg).await?)
    }

    async fn on_bcast_unsubscribes(
        self,
        caller: String,
        msg: UnsubscribedOffersBcast,
    ) -> Result<(), ()> {
        let num_received_ids = msg.offer_ids.len();
        if !msg.offer_ids.is_empty() {
            log::debug!(
                "Received {} unsubscribed Offers from [{}].",
                num_received_ids,
                &caller,
            );
        }

        let offer_unsubscribe_handler = self.inner.offer_unsubscribe_handler.clone();
        let unsubscribed_offer_ids = offer_unsubscribe_handler.call(caller.clone(), msg).await?;

        if !unsubscribed_offer_ids.is_empty() {
            log::debug!(
                "Propagating further {} unsubscribed Offers from {} received from [{}].",
                unsubscribed_offer_ids.len(),
                num_received_ids,
                &caller,
            );

            // No need to retry broadcasting, since we send cyclic broadcasts.
            if let Err(error) = self.bcast_unsubscribes(unsubscribed_offer_ids).await {
                log::error!("Error propagating unsubscribed Offers further: {}", error,);
            }
        }
        Ok(())
    }

    async fn default_identity(&self) -> Result<NodeId, IdentityError> {
        Ok(self.inner.identity.default_identity().await?)
    }
}
