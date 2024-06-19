use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use ya_core_model::net::local::BroadcastMessage;
use ya_core_model::NodeId;
use ya_service_bus::RpcMessage;

use crate::db::model::{Offer as ModelOffer, SubscriptionId};

use super::super::callback::CallbackMessage;
use super::DiscoveryRemoteError;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffersBcast {
    pub offer_ids: Vec<SubscriptionId>,
}

/// Local handler will return only ids of offers, that were not yet known.
/// Those will be retrieved directly from the bcast sender.
impl CallbackMessage for OffersBcast {
    type Ok = Vec<SubscriptionId>;
    type Error = ();
}

impl BroadcastMessage for OffersBcast {
    const TOPIC: &'static str =
        concat!("market-protocol-discovery-", PROTOCOL_VERSION!(), "-offers");
}

pub fn get_offers_addr(prefix: &str) -> String {
    format!(
        "{}/protocol/{}/discovery/offers",
        prefix,
        PROTOCOL_VERSION!()
    )
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrieveOffers {
    pub offer_ids: Vec<SubscriptionId>,
}

impl RpcMessage for RetrieveOffers {
    const ID: &'static str = "Get";
    type Item = Vec<ModelOffer>;
    type Error = DiscoveryRemoteError;
}

/// Asks node for information about
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryOffers {
    pub node_id: Option<NodeId>,
    pub constraint_expr: Option<String>,
    pub iterator: Option<ByteBuf>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryOffersResult {
    pub offers: Vec<SubscriptionId>,
    pub iterator: Option<ByteBuf>,
}

impl RpcMessage for QueryOffers {
    const ID: &'static str = "Query";
    type Item = QueryOffersResult;
    type Error = DiscoveryRemoteError;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffersRetrieved {
    pub offers: Vec<ModelOffer>,
}

/// Local handler will return only ids of offers, that was successfully saved.
/// Those will be bcasted further to the network.
impl CallbackMessage for OffersRetrieved {
    type Ok = Vec<SubscriptionId>;
    type Error = ();
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribedOffersBcast {
    pub offer_ids: Vec<SubscriptionId>,
}

/// Local handler will return only ids of offers, that were not yet known as unsubscribed.
/// Those will be bcasted further to the network.
impl CallbackMessage for UnsubscribedOffersBcast {
    type Ok = Vec<SubscriptionId>;
    type Error = ();
}

impl BroadcastMessage for UnsubscribedOffersBcast {
    const TOPIC: &'static str = concat!(
        "market-protocol-discovery-",
        PROTOCOL_VERSION!(),
        "-offers-unsubscribe"
    );
}
