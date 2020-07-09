use chrono::prelude::*;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

use ya_client::model::ErrorMessage;
use ya_core_model::net;
use ya_core_model::net::local::{BindBroadcastError, BroadcastMessage, SendBroadcastMessage};
use ya_service_bus::{typed as bus, RpcEndpoint};

use crate::db::models::{Offer as ModelOffer, SubscriptionId};
use crate::protocol::{CallbackMessage, HandlerSlot};

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
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryRemoteError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryInitError {
    #[error("Uninitialized callback '{0}'.")]
    UninitializedCallback(String),
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

pub struct DiscoveryImpl {
    offer_received: HandlerSlot<OfferReceived>,
    offer_unsubscribed: HandlerSlot<OfferUnsubscribed>,
    retrieve_offers: HandlerSlot<RetrieveOffers>,
}

impl Discovery {
    /// Broadcasts offer to other nodes in network. Connected nodes will
    /// get call to function bound in `offer_received`.
    pub async fn broadcast_offer(&self, offer: ModelOffer) -> Result<(), DiscoveryError> {
        log::info!("Broadcasting offer [{}].", &offer.id);

        let original_sender = offer.node_id.clone();
        let bcast_msg = SendBroadcastMessage::new(OfferReceived { offer });

        let _ = bus::service(net::local::BUS_ID)
            .send_as(original_sender, bcast_msg) // TODO: should we send as our (default) identity?
            .await?;
        Ok(())
    }

    pub async fn broadcast_unsubscribe(
        &self,
        caller: String,
        subscription_id: SubscriptionId,
    ) -> Result<(), DiscoveryError> {
        log::info!("Broadcasting unsubscribe offer [{}].", &subscription_id);

        let msg = OfferUnsubscribed { subscription_id };
        let bcast_msg = SendBroadcastMessage::new(msg);

        let _ = bus::service(net::local::BUS_ID)
            .send_as(caller, bcast_msg)
            .await?;
        Ok(())
    }

    pub async fn retrieve_offers(&self) -> Result<Vec<ModelOffer>, DiscoveryError> {
        unimplemented!()
    }

    pub async fn bind_gsb(
        &self,
        public_prefix: &str,
        private_prefix: &str,
    ) -> Result<(), DiscoveryInitError> {
        let myself = self.clone();
        // /private/market/market-protocol-mk1-offer
        let broadcast_address = format!("{}/{}", private_prefix, OfferReceived::TOPIC);
        ya_net::bind_broadcast_with_caller(
            &broadcast_address,
            move |caller, msg: SendBroadcastMessage<OfferReceived>| {
                let myself = myself.clone();
                myself.on_offer_received(caller, msg.body().to_owned())
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

        Ok(())
    }

    async fn on_offer_received(self, caller: String, msg: OfferReceived) -> Result<(), ()> {
        let callback = self.inner.offer_received.clone();

        let offer = msg.offer.clone();
        let offer_id = offer.id.clone();
        let provider_id = offer.node_id.clone();

        log::info!(
            "Received broadcasted Offer [{}] from provider [{}]. Sender: [{}].",
            offer_id,
            provider_id,
            &caller,
        );

        match callback.call(caller, msg).await? {
            Propagate::Yes => {
                log::info!("Propagating further Offer [{}].", offer_id,);

                // TODO: Should we retry in case of fail?
                if let Err(error) = self.broadcast_offer(offer).await {
                    log::error!(
                        "Error propagating Offer [{}] from provider [{}] further. Error: {}",
                        offer_id,
                        provider_id,
                        error,
                    );
                }
            }
            Propagate::No(reason) => {
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

        match callback.call(caller.clone(), msg).await? {
            Propagate::Yes => {
                log::info!(
                    "Propagating further unsubscribe Offer [{}].",
                    &subscription_id,
                );

                // TODO: Should we retry in case of fail?
                if let Err(error) = self
                    .broadcast_unsubscribe(caller, subscription_id.clone())
                    .await
                {
                    log::error!(
                        "Error propagating unsubscribe Offer [{}] further. Error: {}",
                        subscription_id,
                        error,
                    );
                }
            }
            Propagate::No(reason) => {
                log::info!(
                    "Not propagating unsubscribe Offer [{}] because: {}.",
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
pub enum Reason {
    #[display(fmt = "Offer already exists in database")]
    AlreadyExists,
    #[display(fmt = "Offer already unsubscribed")]
    Unsubscribed,
    #[display(fmt = "Offer not found in database")]
    NotFound,
    #[display(fmt = "Offer expired")]
    Expired,
    #[display(fmt = "Propagation error: {}", "_0")]
    Error(String),
}

#[derive(Serialize, Deserialize)]
pub enum Propagate {
    Yes,
    No(Reason),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferReceived {
    pub offer: ModelOffer,
}

impl CallbackMessage for OfferReceived {
    type Item = Propagate;
    type Error = ();
}

impl BroadcastMessage for OfferReceived {
    const TOPIC: &'static str = "market-protocol-mk1-offer";
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferUnsubscribed {
    pub subscription_id: SubscriptionId,
}

impl CallbackMessage for OfferUnsubscribed {
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

impl CallbackMessage for RetrieveOffers {
    type Item = Vec<ModelOffer>;
    type Error = DiscoveryRemoteError;
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
