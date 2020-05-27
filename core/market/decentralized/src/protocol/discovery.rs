use async_trait::async_trait;
use chrono::prelude::*;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::marker::Send;
use thiserror::Error;

use ya_core_model::net;
use ya_core_model::net::local::{BroadcastMessage, Subscribe, ToEndpoint};
use ya_client::model::market::Offer;
use ya_service_bus::{typed as bus, RpcMessage, RpcEndpoint};

use super::callbacks::{CallbackHandler, HandlerSlot};

// =========================================== //
// Errors
// =========================================== //

#[derive(Error, Display, Debug, Serialize, Deserialize)]
pub enum DiscoveryError {
    RemoteError(#[from] DiscoveryRemoteError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryRemoteError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryInitError {
    #[error("Uninitialized callback '{}'.", .0)]
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
#[async_trait(?Send)]
pub trait Discovery: Send + Sync {
    async fn bind_gsb(&self, prefix: String) -> Result<(), DiscoveryInitError>;

    /// Broadcasts offer to other nodes in network. Connected nodes will
    /// get call to function bound in DiscoveryBuilder::bind_offer_received.
    async fn broadcast_offer(&self, offer: Offer) -> Result<(), DiscoveryError>;
    async fn retrieve_offers(&self) -> Result<Vec<Offer>, DiscoveryError>;
}

/// Creates Discovery of specific type.
pub trait DiscoveryFactory {
    fn new(builder: DiscoveryBuilder) -> Result<Arc<dyn Discovery>, DiscoveryInitError>;
}

/// Discovery API initialization.
pub struct DiscoveryBuilder {
    offer_received: Option<HandlerSlot<OfferReceived>>,
    retrieve_offers: Option<HandlerSlot<RetrieveOffers>>,
}

impl DiscoveryBuilder {
    pub fn new() -> DiscoveryBuilder {
        DiscoveryBuilder {
            offer_received: None,
            retrieve_offers: None,
        }
    }

    pub fn bind_offer_received(mut self, callback: impl CallbackHandler<OfferReceived>) -> Self {
        self.offer_received = Some(HandlerSlot::new(callback));
        self
    }

    pub fn bind_retrieve_offers(mut self, callback: impl CallbackHandler<RetrieveOffers>) -> Self {
        self.retrieve_offers = Some(HandlerSlot::new(callback));
        self
    }

    pub fn offer_received_handler(
        &mut self,
    ) -> Result<HandlerSlot<OfferReceived>, DiscoveryInitError> {
        let handler =
            self.offer_received
                .take()
                .ok_or(DiscoveryInitError::UninitializedCallback(format!(
                    "offer_received"
                )))?;
        Ok(handler)
    }

    pub fn retrieve_offers_handler(
        &mut self,
    ) -> Result<HandlerSlot<RetrieveOffers>, DiscoveryInitError> {
        let handler =
            self.retrieve_offers
                .take()
                .ok_or(DiscoveryInitError::UninitializedCallback(format!(
                    "retrieve_offers"
                )))?;
        Ok(handler)
    }

    pub fn build<Factory: DiscoveryFactory>(
        self,
    ) -> Result<Arc<dyn Discovery>, DiscoveryInitError> {
        Ok(Factory::new(self)?)
    }
}

// =========================================== //
// Discovery implementation
// =========================================== //

/// Implementation of Discovery protocol using GSB.
pub struct DiscoveryGSB {
    offer_received: HandlerSlot<OfferReceived>,
    retrieve_offers: HandlerSlot<RetrieveOffers>,
}

impl DiscoveryFactory for DiscoveryGSB {
    fn new(mut builder: DiscoveryBuilder) -> Result<Arc<dyn Discovery>, DiscoveryInitError> {
        let offer_received = builder.offer_received_handler()?;
        let retrieve_offers = builder.retrieve_offers_handler()?;

        Ok(Arc::new(DiscoveryGSB {
            offer_received,
            retrieve_offers,
        }))
    }
}

#[async_trait(?Send)]
impl Discovery for DiscoveryGSB {
    async fn broadcast_offer(&self, offer: Offer) -> Result<(), DiscoveryError> {
        // TODO: Implement
        Ok(())
    }

    async fn retrieve_offers(&self) -> Result<Vec<Offer>, DiscoveryError> {
        unimplemented!()
    }

    async fn bind_gsb(&self, prefix: String) -> Result<(), DiscoveryInitError> {
        let retrieve_handler = self.retrieve_offers.clone();
        let offer_received_handler = self.offer_received.clone();

        let offer_broadcast_address = format!("{}/{}", prefix, OfferReceived::TOPIC);
        let subscribe_msg = OfferReceived::into_subscribe_msg(&offer_broadcast_address);
        bus::service(net::BUS_ID).send(subscribe_msg).await??;

        let _ = bus::bind_with_caller(&offer_broadcast_address, move |caller, msg: OfferReceived| {
            let handler = offer_received_handler.clone();
            async move { handler.call(caller, msg).await }
        });

        let _ = bus::bind_with_caller(&prefix, move |caller, msg: RetrieveOffers| {
            let handler = retrieve_handler.clone();
            async move { handler.call(caller, msg).await }
        });

        Ok(())
    }
}

// =========================================== //
// Discovery messages
// =========================================== //

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferReceived {
    pub offer: Offer,
}

impl RpcMessage for OfferReceived {
    const ID: &'static str = "OfferReceived";
    type Item = ();
    type Error = DiscoveryRemoteError;
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
// Internal Discovery messages used
// for communication between market instances
// of Discovery protocol
// =========================================== //

impl BroadcastMessage for OfferReceived {
    const TOPIC: &'static str = "market/protocol/mk1/offer";
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

