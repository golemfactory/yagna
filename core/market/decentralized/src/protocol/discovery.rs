// TODO: This is only temporary
#![allow(dead_code)]
use chrono::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

use ya_client::model::ErrorMessage;
use ya_client::model::NodeId;
use ya_core_model::net::local::{BindBroadcastError, BroadcastMessage, SendBroadcastMessage};
use ya_core_model::{identity, net::local as broadcast};
use ya_net::{self as net, RemoteEndpoint};
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::db::model::{Offer as ModelOffer, SubscriptionId};

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
    #[error("Can't get default identity: {0}.")]
    Identity(String),
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

pub struct DiscoveryImpl {
    filter_unknown_offers: HandlerSlot<OfferIdsReceived>,
    offers_received: HandlerSlot<OffersReceived>,
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
        let default_id = default_identity().await?;
        let bcast_msg = SendBroadcastMessage::new(OfferIdsReceived { offers });

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

        Ok(net::from(default_identity().await?)
            .to(target_node)
            .service(&get_offers_addr(BUS_ID))
            .send(GetOffers { offers })
            .await??)
    }

    pub async fn broadcast_unsubscribe(
        &self,
        caller: String,
        offer_id: SubscriptionId,
    ) -> Result<(), DiscoveryError> {
        log::info!("Broadcasting unsubscribe offer [{}].", &offer_id);

        let msg = OfferUnsubscribed { offer_id };
        let bcast_msg = SendBroadcastMessage::new(msg);

        let _ = bus::service(broadcast::BUS_ID)
            .send_as(caller, bcast_msg)
            .await?;
        Ok(())
    }

    pub async fn bind_gsb(
        &self,
        _public_prefix: &str,
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

        ServiceBinder::new(&get_offers_addr(BUS_ID), &(), self.clone()).bind_with_processor(
            move |_, myself, caller: String, msg: GetOffers| {
                let myself = myself.clone();
                myself.on_get_offers(caller, msg)
            },
        );

        Ok(())
    }

    async fn on_offers_received(self, caller: String, msg: OfferIdsReceived) -> Result<(), ()> {
        let filter_callback = self.inner.filter_unknown_offers.clone();
        let offer_received_callback = self.inner.offers_received.clone();

        // TODO: Do this under lock.
        // We should do filtering and getting Offers in single transaction. Otherwise multiple
        // broadcasts can overlap and we will ask other nodes for the same Offers more than once.
        // Note that it wouldn't cause incorrect behavior, because we will add Offers only once.
        // Other attempts to add them will end with error and we will filter all Offers, that already
        // occurred and re-broadcast only new ones.
        // But still it is worth to limit network traffic.
        let unseen_subscriptions = filter_callback.call(caller.clone(), msg).await?;
        let offers = self
            .get_offers(caller.clone(), unseen_subscriptions)
            .await
            .map_err(|e| log::error!("Can't get Offers from [{}]. Error: {}", &caller, e))?;

        let new_ids = offer_received_callback
            .call(caller.clone(), OffersReceived { offers })
            .await?;

        self.broadcast_offers(new_ids).await;
        Ok(())
    }

    async fn on_get_offers(
        self,
        caller: String,
        msg: GetOffers,
    ) -> Result<Vec<ModelOffer>, DiscoveryRemoteError> {
        log::info!("[{}] tries to get Offers from us.", &caller);
        let callback = self.inner.get_offers_request;
        Ok(callback.call(caller, msg).await?)
    }

    async fn on_offer_unsubscribed(self, caller: String, msg: OfferUnsubscribed) -> Result<(), ()> {
        let callback = self.inner.offer_unsubscribed.clone();
        let offer_id = msg.offer_id.clone();

        log::info!(
            "Received broadcasted unsubscribe Offer [{}]. Sender: [{}].",
            offer_id,
            &caller,
        );

        match callback.call(caller.clone(), msg).await? {
            Propagate::Yes => {
                log::info!("Propagating further unsubscribe Offer [{}].", &offer_id,);

                // TODO: Should we retry in case of fail?
                if let Err(error) = self.broadcast_unsubscribe(caller, offer_id.clone()).await {
                    log::error!(
                        "Error propagating unsubscribe Offer [{}] further. Error: {}",
                        offer_id,
                        error,
                    );
                }
            }
            Propagate::No(reason) => {
                log::info!(
                    "Not propagating unsubscribe Offer [{}] because: {}.",
                    offer_id,
                    reason
                );
            }
        }
        Ok(())
    }
}

async fn default_identity() -> Result<NodeId, DiscoveryError> {
    Ok(bus::service(identity::BUS_ID)
        .send(identity::Get::ByDefault)
        .await?
        .map_err(|e| DiscoveryError::Identity(e.to_string()))?
        .ok_or(DiscoveryError::Identity(format!("No default identity!!!")))?
        .node_id)
}

// =========================================== //
// Discovery messages
// =========================================== //

#[derive(Serialize, Deserialize, derive_more::Display)]
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
pub struct OfferIdsReceived {
    pub offers: Vec<SubscriptionId>,
}

impl CallbackMessage for OfferIdsReceived {
    type Item = Vec<SubscriptionId>;
    type Error = ();
}

impl BroadcastMessage for OfferIdsReceived {
    const TOPIC: &'static str = "market-protocol-mk1-offers";
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffersReceived {
    pub offers: Vec<ModelOffer>,
}

impl CallbackMessage for OffersReceived {
    type Item = Vec<SubscriptionId>;
    type Error = ();
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetOffers {
    pub offers: Vec<SubscriptionId>,
}

impl CallbackMessage for GetOffers {
    type Item = Vec<ModelOffer>;
    type Error = DiscoveryRemoteError;
}

impl RpcMessage for GetOffers {
    const ID: &'static str = "Get";
    type Item = Vec<ModelOffer>;
    type Error = DiscoveryRemoteError;
}

fn get_offers_addr(prefix: &str) -> String {
    format!("{}/protocol/mk1/offers/", prefix)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferUnsubscribed {
    pub offer_id: SubscriptionId,
}

impl CallbackMessage for OfferUnsubscribed {
    type Item = Propagate;
    type Error = ();
}

impl BroadcastMessage for OfferUnsubscribed {
    const TOPIC: &'static str = "market-protocol-mk1-offers-unsubscribe";
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
