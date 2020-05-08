use async_trait::async_trait;
use derive_more::Display;
use futures::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use ya_client_model::market::Offer;
use ya_service_bus::RpcMessage;

use super::callbacks::{HandlerSlot, CallbackHandler};


// =========================================== //
// Errors
// =========================================== //

#[derive(Error, Display, Debug, Serialize, Deserialize)]
pub enum DiscoveryError {
    RemoteError(#[from] DiscoveryRemoteError)
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryRemoteError {

}

// =========================================== //
// Discovery interface
// =========================================== //

/// Responsible for communication with markets on other nodes
/// during discovery phase.
#[async_trait]
pub trait Discovery {
    /// Broadcasts offer to other nodes in network.
    async fn broadcast_offer(&self, offer: Offer) -> Result<(), DiscoveryError>;
    async fn retrieve_offers(&self) -> Result<Vec<Offer>, DiscoveryError>;
}

/// Discovery API initialization.
pub struct DiscoveryBuilder {
    offers_receiver: Option<HandlerSlot<OfferReceived>>,
}

impl DiscoveryBuilder {
    pub fn new() -> DiscoveryBuilder {
        DiscoveryBuilder{
            offers_receiver: None,
        }
    }

    pub fn offer_received(mut self, callback: impl CallbackHandler<OfferReceived>) -> Self {
        self.offers_receiver = Some(HandlerSlot::new(callback));
        self
    }
}


// =========================================== //
// Discovery messages
// =========================================== //

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferReceived {
    offer: Offer,
}

impl RpcMessage for OfferReceived {
    const ID: &'static str = "market::OfferReceived";
    type Item = ();
    type Error = DiscoveryRemoteError;
}

