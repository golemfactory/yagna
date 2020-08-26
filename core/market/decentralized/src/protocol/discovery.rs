use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

use ya_client::model::ErrorMessage;
use ya_client::model::NodeId;
use ya_core_model::net::local as broadcast;
use ya_core_model::net::local::{BindBroadcastError, BroadcastMessage, SendBroadcastMessage};
use ya_net::{self as net, RemoteEndpoint};
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::db::model::{Offer as ModelOffer, SubscriptionId};
use crate::identity::{IdentityApi, IdentityError};

use super::callback::{CallbackMessage, HandlerSlot};
use std::str::FromStr;
use ya_core_model::market::BUS_ID;
use ya_service_bus::typed::ServiceBinder;

pub mod builder;

// =========================================== //
// Errors
// =========================================== //

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryError {
    #[error(transparent)]
    RemoteError(#[from] DiscoveryRemoteError),
    #[error("Failed to broadcast caused by gsb error: {0}.")]
    GsbError(String),
    #[error("Internal error: {0}.")]
    InternalError(String),
    #[error(transparent)]
    Identity(#[from] IdentityError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryRemoteError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryInitError {
    #[error("Failed to bind broadcast `{0}` to gsb. Error: {1}.")]
    BindingGsbFailed(String, String),
    #[error("Failed to subscribe to broadcast `{0}`. Error: {1}.")]
    BroadcastSubscribeFailed(String, String),
}

// =========================================== //
// Discovery interface
// =========================================== //

/// Responsible for communication with markets on other nodes
/// during discovery phase.
#[derive(Clone)]
pub struct Discovery {
    inner: Arc<DiscoveryImpl>,
}

pub(super) struct ReceiveHandlers {
    offer_ids: HandlerSlot<OfferIdsReceived>,
    offers: HandlerSlot<OffersReceived>,
}

pub struct DiscoveryImpl {
    identity: Arc<dyn IdentityApi>,

    receive: Mutex<ReceiveHandlers>,
    offer_unsubscribed: HandlerSlot<OfferUnsubscribed>,
    get_offers_request: HandlerSlot<GetOffers>,
}

impl Discovery {
    /// Broadcasts offer to other nodes in network. Connected nodes will
    /// get call to function bound in `offer_received`.
    pub async fn broadcast_offers(
        &self,
        offers: Vec<SubscriptionId>,
    ) -> Result<(), DiscoveryError> {
        let default_id = self.default_identity().await?;
        let bcast_msg = SendBroadcastMessage::new(OfferIdsReceived { offers });

        // TODO: We shouldn't use send_as. Put identity inside broadcasted message instead.
        let _ = bus::service(broadcast::BUS_ID)
            .send_as(default_id, bcast_msg) // TODO: should we send as our (default) identity?
            .await?;
        Ok(())
    }

    /// Ask remote Node for specified Offers.
    pub async fn get_offers(
        &self,
        from: String,
        offers: Vec<SubscriptionId>,
    ) -> Result<Vec<ModelOffer>, DiscoveryError> {
        let target_node =
            NodeId::from_str(&from).map_err(|e| DiscoveryError::InternalError(e.to_string()))?;

        Ok(net::from(self.default_identity().await?)
            .to(target_node)
            .service(&get_offers_addr(BUS_ID))
            .send(GetOffers { offers })
            .await??)
    }

    pub async fn broadcast_unsubscribes(
        &self,
        offers: Vec<SubscriptionId>,
    ) -> Result<(), DiscoveryError> {
        let default_id = self.default_identity().await?;

        let msg = OfferUnsubscribed { offers };
        let bcast_msg = SendBroadcastMessage::new(msg);

        // TODO: We shouldn't use send_as. Put identity inside broadcasted message instead.
        let _ = bus::service(broadcast::BUS_ID)
            .send_as(default_id, bcast_msg)
            .await?;
        Ok(())
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), DiscoveryInitError> {
        let myself = self.clone();
        // /private/market/market-protocol-mk1-offer
        let broadcast_address = format!("{}/{}", private_prefix, OfferIdsReceived::TOPIC);
        ya_net::bind_broadcast_with_caller(
            &broadcast_address,
            move |caller, msg: SendBroadcastMessage<OfferIdsReceived>| {
                let myself = myself.clone();
                myself.on_offers_received(caller, msg.body().to_owned())
            },
        )
        .await
        .map_err(|e| DiscoveryInitError::from_pair(broadcast_address, e))?;

        let myself = self.clone();
        // /private/market/market-protocol-mk1-offer-unsubscribe
        let broadcast_address = format!("{}/{}", private_prefix, OfferUnsubscribed::TOPIC);
        ya_net::bind_broadcast_with_caller(
            &broadcast_address,
            move |caller, msg: SendBroadcastMessage<OfferUnsubscribed>| {
                let myself = myself.clone();
                myself.on_offer_unsubscribed(caller, msg.body().to_owned())
            },
        )
        .await
        .map_err(|e| DiscoveryInitError::from_pair(broadcast_address, e))?;

        ServiceBinder::new(&get_offers_addr(public_prefix), &(), self.clone()).bind_with_processor(
            move |_, myself, caller: String, msg: GetOffers| {
                let myself = myself.clone();
                myself.on_get_offers(caller, msg)
            },
        );

        Ok(())
    }

    async fn on_offers_received(self, caller: String, msg: OfferIdsReceived) -> Result<(), ()> {
        let num_ids_received = msg.offers.len();
        if !msg.offers.is_empty() {
            log::debug!("Received {} Offers from [{}].", num_ids_received, &caller);
        }

        // We should do filtering and getting Offers in single transaction. Otherwise multiple
        // broadcasts can overlap and we will ask other nodes for the same Offers more than once.
        // Note that it wouldn't cause incorrect behavior, because we will add Offers only once.
        // Other attempts to add them will end with error and we will filter all Offers, that already
        // occurred and re-broadcast only new ones.
        // But still it is worth to limit network traffic.
        let new_ids = {
            let receive_handlers = self.inner.receive.lock().await;
            let filter_callback = receive_handlers.offer_ids.clone();
            let offer_received_callback = receive_handlers.offers.clone();

            let unseen_subscriptions = filter_callback.call(caller.clone(), msg).await?;

            if !unseen_subscriptions.is_empty() {
                let offers = self
                    .get_offers(caller.clone(), unseen_subscriptions)
                    .await
                    .map_err(|e| log::warn!("Can't get Offers from [{}]. Error: {}", &caller, e))?;

                // We still could fail to add some Offers to database. If we fail to add them, we don't
                // want to propagate subscription further.
                offer_received_callback
                    .call(caller.clone(), OffersReceived { offers })
                    .await?
            } else {
                vec![]
            }
        };

        if !new_ids.is_empty() {
            log::info!(
                "Propagating {}/{} Offers received from [{}].",
                new_ids.len(),
                num_ids_received,
                &caller
            );

            // We could broadcast outside of lock, but it shouldn't hurt either, because
            // we don't wait for any responses from remote nodes.
            self.broadcast_offers(new_ids)
                .await
                .map_err(|e| log::warn!("Failed to broadcast. Error: {}", e))?;
        }

        Ok(())
    }

    async fn on_get_offers(
        self,
        caller: String,
        msg: GetOffers,
    ) -> Result<Vec<ModelOffer>, DiscoveryRemoteError> {
        log::info!("[{}] tries to get Offers from us.", &caller);
        let callback = self.inner.get_offers_request.clone();
        Ok(callback.call(caller, msg).await?)
    }

    async fn on_offer_unsubscribed(self, caller: String, msg: OfferUnsubscribed) -> Result<(), ()> {
        let num_received_ids = msg.offers.len();
        if !msg.offers.is_empty() {
            log::debug!(
                "Received {} unsubscribed Offers from [{}].",
                num_received_ids,
                &caller,
            );
        }

        let callback = self.inner.offer_unsubscribed.clone();
        let subscriptions = callback.call(caller.clone(), msg).await?;

        if !subscriptions.is_empty() {
            log::info!(
                "Propagating further {} unsubscribed Offers from {} received from [{}].",
                subscriptions.len(),
                num_received_ids,
                &caller,
            );

            // No need to retry broadcasting, since we send cyclic broadcasts.
            if let Err(error) = self.broadcast_unsubscribes(subscriptions).await {
                log::error!("Error propagating unsubscribed Offers further: {}", error,);
            }
        }
        Ok(())
    }

    async fn default_identity(&self) -> Result<NodeId, IdentityError> {
        Ok(self.inner.identity.default_identity().await?)
    }
}

// =========================================== //
// Discovery messages
// =========================================== //

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferIdsReceived {
    pub offers: Vec<SubscriptionId>,
}

impl CallbackMessage for OfferIdsReceived {
    type Item = Vec<SubscriptionId>;
    type Error = ();
}

impl BroadcastMessage for OfferIdsReceived {
    const TOPIC: &'static str = "market-protocol-discovery-mk1-offers";
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffersReceived {
    pub offers: Vec<ModelOffer>,
}

impl CallbackMessage for OffersReceived {
    /// Callback handler should return all subscription ids, that were new to him
    /// and should be propagated further to the network.
    type Item = Vec<SubscriptionId>;
    type Error = ();
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetOffers {
    pub offers: Vec<SubscriptionId>,
}

impl RpcMessage for GetOffers {
    const ID: &'static str = "Get";
    type Item = Vec<ModelOffer>;
    type Error = DiscoveryRemoteError;
}

fn get_offers_addr(prefix: &str) -> String {
    format!("{}/protocol/discovery/mk1/offers", prefix)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferUnsubscribed {
    pub offers: Vec<SubscriptionId>,
}

impl CallbackMessage for OfferUnsubscribed {
    /// Callback handler should return all subscription ids, that were new to him
    /// and should be propagated further to the network.
    type Item = Vec<SubscriptionId>;
    type Error = ();
}

impl BroadcastMessage for OfferUnsubscribed {
    const TOPIC: &'static str = "market-protocol-discovery-mk1-offers-unsubscribe";
}

// =========================================== //
// Errors From impls
// =========================================== //

impl DiscoveryInitError {
    fn from_pair(addr: String, e: BindBroadcastError) -> Self {
        match e {
            BindBroadcastError::GsbError(e) => {
                DiscoveryInitError::BindingGsbFailed(addr, e.to_string())
            }
            BindBroadcastError::SubscribeError(e) => {
                DiscoveryInitError::BroadcastSubscribeFailed(addr, e.to_string())
            }
        }
    }
}

impl From<ya_service_bus::error::Error> for DiscoveryError {
    fn from(e: ya_service_bus::error::Error) -> Self {
        DiscoveryError::GsbError(e.to_string())
    }
}

impl From<ErrorMessage> for DiscoveryError {
    fn from(e: ErrorMessage) -> Self {
        DiscoveryError::InternalError(e.to_string())
    }
}
