use async_trait::async_trait;
use chrono::prelude::*;
use derive_more::Display;
use futures::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use ya_client_model::market::Offer;
use ya_service_bus::RpcMessage;

use super::callbacks::{CallbackHandler, HandlerSlot};

// =========================================== //
// Errors
// =========================================== //

#[derive(Error, Display, Debug, Serialize, Deserialize)]
pub enum DiscoveryError {
    RemoteError(#[from] DiscoveryRemoteError),
    InitializationFailed(#[from] InitError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryRemoteError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum InitError {
    #[error("Uninitialized callback '{}'.", .0)]
    UninitializedCallback(String),
}

// =========================================== //
// Discovery interface
// =========================================== //

/// Responsible for communication with markets on other nodes
/// during discovery phase.
#[async_trait]
pub trait Discovery {
    /// Broadcasts offer to other nodes in network. Connected nodes will
    /// get call to function bound in DiscoveryBuilder::bind_offer_received.
    async fn broadcast_offer(&self, offer: Offer) -> Result<(), DiscoveryError>;
    async fn retrieve_offers(&self) -> Result<Vec<Offer>, DiscoveryError>;
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

    pub fn offer_received_handler(&mut self) -> Result<HandlerSlot<OfferReceived>, InitError> {
        let handler = self
            .offer_received
            .take()
            .ok_or(InitError::UninitializedCallback(format!("offer_received")))?;
        Ok(handler)
    }

    pub fn retrieve_offers_handler(&mut self) -> Result<HandlerSlot<RetrieveOffers>, InitError> {
        let handler = self
            .retrieve_offers
            .take()
            .ok_or(InitError::UninitializedCallback(format!("retrieve_offers")))?;
        Ok(handler)
    }
}

// =========================================== //
// Discovery implementation
// =========================================== //

/// Implementation of Discovery protocol using GSB.
struct DiscoveryImpl {
    offer_received: HandlerSlot<OfferReceived>,
    retrieve_offers: HandlerSlot<RetrieveOffers>,
}

impl DiscoveryImpl {
    pub fn new(mut builder: DiscoveryBuilder) -> Result<DiscoveryImpl, DiscoveryError> {
        let offer_received = builder.offer_received_handler()?;
        let retrieve_offers = builder.retrieve_offers_handler()?;

        Ok(DiscoveryImpl {
            offer_received,
            retrieve_offers,
        })
    }
}

#[async_trait]
impl Discovery for DiscoveryImpl {
    async fn broadcast_offer(&self, offer: Offer) -> Result<(), DiscoveryError> {
        unimplemented!()
    }

    async fn retrieve_offers(&self) -> Result<Vec<Offer>, DiscoveryError> {
        unimplemented!()
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
    const ID: &'static str = "market::OfferReceived";
    type Item = ();
    type Error = DiscoveryRemoteError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrieveOffers {
    pub newer_than: chrono::DateTime<Utc>,
}

impl RpcMessage for RetrieveOffers {
    const ID: &'static str = "market::RetrieveOffers";
    type Item = Vec<Offer>;
    type Error = DiscoveryRemoteError;
}
