use chrono::prelude::*;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::marker::Send;
use std::sync::Arc;
use thiserror::Error;

use ya_client::model::market::Offer;
use ya_client::model::ErrorMessage;
use ya_core_model::net;
use ya_core_model::net::local::{BroadcastMessage, SendBroadcastMessage, Subscribe, ToEndpoint};
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use super::callbacks::{CallbackHandler, HandlerSlot};

// =========================================== //
// Errors
// =========================================== //

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryError {
    #[error(transparent)]
    RemoteError(#[from] DiscoveryRemoteError),
    #[error("Failed to broadcast caused by gsb error: {}.", .0)]
    GsbError(String),
    #[error("Internal error: {}.", .0)]
    InternalError(String),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryRemoteError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryInitError {
    #[error("Uninitialized callback '{0}'.")]
    UninitializedCallback(String),
    #[error("Failed to bind to gsb. Error: {}.", .0)]
    BindingGsbFailed(String),
    #[error("Failed to subscribe to broadcast. Error: {0}.")]
    BroadcastSubscribeFailed(String),
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

pub struct DiscoveryImpl {
    offer_received: HandlerSlot<OfferReceived>,
    offer_unsubscribed: HandlerSlot<OfferUnsubscribed>,
    retrieve_offers: HandlerSlot<RetrieveOffers>,
}

impl Discovery {
    pub fn new(
        offer_received: impl CallbackHandler<OfferReceived>,
        offer_unsubscribed: impl CallbackHandler<OfferUnsubscribed>,
        retrieve_offers: impl CallbackHandler<RetrieveOffers>,
    ) -> Result<Discovery, DiscoveryInitError> {
        let inner = Arc::new(DiscoveryImpl {
            offer_received: HandlerSlot::new(offer_received),
            offer_unsubscribed: HandlerSlot::new(offer_unsubscribed),
            retrieve_offers: HandlerSlot::new(retrieve_offers),
        });
        Ok(Discovery { inner })
    }

    /// Broadcasts offer to other nodes in network. Connected nodes will
    /// get call to function bound in DiscoveryBuilder::bind_offer_received.
    pub async fn broadcast_offer(&self, offer: Offer) -> Result<(), DiscoveryError> {
        log::info!("Broadcasting offer [{}] to the network.", offer.offer_id()?);

        let msg = OfferReceived { offer };
        let bcast_msg = SendBroadcastMessage::new(msg);

        let _ = bus::service(net::local::BUS_ID).send(bcast_msg).await?;
        Ok(())
    }

    pub async fn broadcast_unsubscribe(
        &self,
        subscription_id: String,
    ) -> Result<(), DiscoveryError> {
        log::info!(
            "Broadcasting unsubscribe offer [{}] to the network.",
            &subscription_id
        );

        let msg = OfferUnsubscribed { subscription_id };
        let bcast_msg = SendBroadcastMessage::new(msg);

        let _ = bus::service(net::local::BUS_ID).send(bcast_msg).await?;
        Ok(())
    }

    pub async fn retrieve_offers(&self) -> Result<Vec<Offer>, DiscoveryError> {
        unimplemented!()
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), DiscoveryInitError> {
        let myself = self.clone();

        log::debug!("Creating broadcast topic {}.", OfferReceived::TOPIC);

        let offer_broadcast_address = format!("{}/{}", private_prefix, OfferReceived::TOPIC);
        let subscribe_msg = OfferReceived::into_subscribe_msg(&offer_broadcast_address);
        bus::service(net::local::BUS_ID)
            .send(subscribe_msg)
            .await??;

        log::debug!(
            "Binding handler for broadcast topic {}.",
            OfferReceived::TOPIC
        );

        let _ = bus::bind_with_caller(
            &offer_broadcast_address,
            move |caller, msg: SendBroadcastMessage<OfferReceived>| {
                let myself = myself.clone();
                myself.on_offer_received(caller, msg.body().to_owned())
            },
        );

        log::debug!("Creating broadcast topic {}.", OfferUnsubscribed::TOPIC);

        let unsubscribe_broadcast_address =
            format!("{}/{}", private_prefix, OfferUnsubscribed::TOPIC);
        let subscribe_msg = OfferUnsubscribed::into_subscribe_msg(&unsubscribe_broadcast_address);
        bus::service(net::local::BUS_ID)
            .send(subscribe_msg)
            .await??;

        log::debug!(
            "Binding handler for broadcast topic {}.",
            OfferUnsubscribed::TOPIC
        );

        let myself = self.clone();
        let _ = bus::bind_with_caller(
            &unsubscribe_broadcast_address,
            move |caller, msg: SendBroadcastMessage<OfferUnsubscribed>| {
                let myself = myself.clone();
                myself.on_offer_unsubscribed(caller, msg.body().to_owned())
            },
        );

        Ok(())
    }

    async fn on_offer_received(self, caller: String, msg: OfferReceived) -> Result<(), ()> {
        let callback = self.inner.offer_received.clone();

        let offer = msg.offer.clone();
        let offer_id = offer.offer_id().unwrap_or("{Empty id}").to_string();
        let provider_id = offer.provider_id().unwrap_or("{Empty id}").to_string();

        log::info!(
            "Received broadcasted Offer [{}] from provider [{}]. Sender: [{}].",
            offer_id,
            provider_id,
            &caller,
        );

        match callback.call(caller, msg).await? {
            Propagate::True => {
                log::info!("Propagating further Offer [{}].", offer_id,);

                // TODO: Should we retry in case of fail?
                if let Err(error) = self.broadcast_offer(offer).await {
                    log::error!(
                        "Error propagating further Offer [{}] from provider [{}].",
                        offer_id,
                        provider_id
                    );
                }
            }
            Propagate::False(reason) => {
                log::info!(
                    "Not propagating Offer [{}] for reason: {}.",
                    offer_id,
                    reason
                );
            }
        }
        Ok(())
    }

    async fn on_offer_unsubscribed(self, caller: String, msg: OfferUnsubscribed) -> Result<(), ()> {
        let callback = self.inner.offer_unsubscribed.clone();
        let subscription_id = msg.subscription_id.clone();

        log::info!(
            "Received broadcasted unsubscribe Offer [{}]. Sender: [{}].",
            subscription_id,
            &caller,
        );

        match callback.call(caller, msg).await? {
            Propagate::True => {
                log::info!(
                    "Propagating further unsubscribe Offer [{}].",
                    &subscription_id,
                );

                // TODO: Should we retry in case of fail?
                if let Err(error) = self.broadcast_unsubscribe(subscription_id.clone()).await {
                    log::error!(
                        "Error propagating further unsubscribe Offer [{}].",
                        subscription_id,
                    );
                }
            }
            Propagate::False(reason) => {
                log::info!(
                    "Not propagating unsubscribe Offer [{}] for reason: {}.",
                    subscription_id,
                    reason
                );
            }
        }
        Ok(())
    }
}

// =========================================== //
// Discovery messages
// =========================================== //

#[derive(Serialize, Deserialize, Display)]
pub enum StopPropagateReason {
    #[display(fmt = "Offer already exists in database")]
    AlreadyExists,
    #[display(fmt = "Error adding offer: {}", "_0")]
    Error(String),
    #[display(fmt = "Offer already unsubscribed")]
    AlreadyUnsubscribed,
}

#[derive(Serialize, Deserialize)]
pub enum Propagate {
    True,
    False(StopPropagateReason),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferReceived {
    pub offer: Offer,
}

impl RpcMessage for OfferReceived {
    const ID: &'static str = "OfferReceived";
    type Item = Propagate;
    type Error = ();
}

impl BroadcastMessage for OfferReceived {
    const TOPIC: &'static str = "market-protocol-mk1-offer";
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferUnsubscribed {
    pub subscription_id: String,
}

impl RpcMessage for OfferUnsubscribed {
    const ID: &'static str = "OfferUnsubscribed";
    type Item = Propagate;
    type Error = ();
}

impl BroadcastMessage for OfferUnsubscribed {
    const TOPIC: &'static str = "market-protocol-mk1-offer-unsubscribe";
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrieveOffers {
    pub newer_than: chrono::DateTime<Utc>,
}

impl RpcMessage for RetrieveOffers {
    const ID: &'static str = "RetrieveOffers";
    type Item = Vec<Offer>;
    type Error = DiscoveryRemoteError;
}

// =========================================== //
// Errors From impls
// =========================================== //

impl From<net::local::SubscribeError> for DiscoveryInitError {
    fn from(err: net::local::SubscribeError) -> Self {
        DiscoveryInitError::BroadcastSubscribeFailed(format!("{}", err))
    }
}

impl From<ya_service_bus::error::Error> for DiscoveryInitError {
    fn from(err: ya_service_bus::error::Error) -> Self {
        DiscoveryInitError::BindingGsbFailed(format!("{}", err))
    }
}

impl From<ya_service_bus::error::Error> for DiscoveryError {
    fn from(err: ya_service_bus::error::Error) -> Self {
        DiscoveryError::GsbError(format!("{}", err))
    }
}

impl From<ErrorMessage> for DiscoveryError {
    fn from(e: ErrorMessage) -> Self {
        DiscoveryError::InternalError(e.to_string())
    }
}
