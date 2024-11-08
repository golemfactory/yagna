// YA_NET_BIND_URL=udp://0.0.0.0:11500
//

use anyhow::Result;
use bytes::Bytes;
use ethsign::{PublicKey, Signature};
use futures::future::LocalBoxFuture;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use ya_core_model::NodeId;

///
/// let client = NetClient::builder()
///     .bind_url("udp://0.0.0.0:11500")
///     .crypto_provider(provider)
///     .start().await?;
///

pub struct NetClientBuilder {
    _inner: (),
}

impl NetClientBuilder {}

pub struct NetClient {
    _inner: (),
}

impl NetClient {
    pub async fn send_msg(&self, from: NodeId, to: NodeId, msg: Bytes) -> Result<()> {
        todo!()
    }

    pub async fn recv_msg(&self) -> Result<(NodeId, NodeId, Bytes)> {
        todo!()
    }

    pub async fn send_unreliable_msg(&self, from: NodeId, to: NodeId, msg: Bytes) -> Result<()> {
        todo!()
    }

    pub async fn recv_unreliable_msg(&self) -> Result<(NodeId, NodeId, Bytes)> {
        todo!()
    }

    pub async fn find_mtu(&self, to: NodeId) -> Result<usize> {
        todo!()
    }

    pub async fn send_broadcast(
        &self,
        from: NodeId,
        topic: String,
        size: usize,
    ) -> Result<(NodeId, NodeId, Bytes)> {
        todo!()
    }

    pub async fn recv_broadcast(&self, topic: String) -> impl Stream<Item = (NodeId, Bytes)> {
        todo!()
    }

    pub async fn status(&self) -> Result<NetStatus> {
        todo!()
    }
}

#[derive(Serialize, Deserialize)]
pub struct NetStatus {}

pub trait CryptoProvider {
    fn default_id<'a>(&self) -> LocalBoxFuture<'a, anyhow::Result<NodeId>>;
    fn aliases<'a>(&self) -> LocalBoxFuture<'a, anyhow::Result<Vec<NodeId>>>;
    fn get<'a>(&self, node_id: NodeId) -> LocalBoxFuture<'a, anyhow::Result<Rc<dyn Crypto>>>;
}

pub trait Crypto {
    fn public_key<'a>(&self) -> LocalBoxFuture<'a, anyhow::Result<PublicKey>>;
    fn sign<'a>(&self, message: &'a [u8]) -> LocalBoxFuture<'a, anyhow::Result<Signature>>;
    fn encrypt<'a>(
        &self,
        message: &'a [u8],
        remote_key: &'a PublicKey,
    ) -> LocalBoxFuture<'a, anyhow::Result<Vec<u8>>>;
}
