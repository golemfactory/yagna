use std::net::SocketAddr;
use std::sync::Arc;

use actix::prelude::*;
use anyhow::{anyhow, bail};
use std::sync::RwLock;

use ya_core_model::NodeId;
use ya_relay_client::{ChannelMetrics, Client, SessionDesc, SocketDesc, SocketState};

lazy_static::lazy_static! {
    static ref ADDRESS: Arc<RwLock<Option<Addr<ClientActor >>>> = Default::default();
}

#[derive(Clone)]
pub struct ClientProxy(Addr<ClientActor>);

impl ClientProxy {
    pub fn new() -> anyhow::Result<Self> {
        let addr = match ADDRESS.read().unwrap().clone() {
            Some(addr) => addr,
            None => bail!("Net client not initialized. ClientProxy has no address of ClientActor"),
        };

        Ok(Self(addr))
    }

    async fn call<M>(&self, msg: M) -> anyhow::Result<M::Result>
    where
        M: Message + Send + 'static,
        <M as Message>::Result: Send + 'static,
        ClientActor: Handler<M>,
    {
        let resp = self
            .0
            .send(msg)
            .await
            .map_err(|_| anyhow!("network not running"))?;
        Ok(resp)
    }
}

pub(crate) struct ClientActor {
    client: Client,
}

impl ClientActor {
    /// Creates and starts `ClientActor`, and then shares its address with `ClientProxy`.
    pub fn init(client: Client) {
        let client = Self { client };
        let addr = client.start();
        ADDRESS.write().unwrap().replace(addr);
    }
}

impl Actor for ClientActor {
    type Context = Context<Self>;
}

macro_rules! proxy {
    ($ident:ident -> $ty:ty, $by:ident, $handler:expr) => {
        #[derive(Default)]
        struct $ident;

        impl Message for $ident {
            type Result = $ty;
        }

        impl Handler<$ident> for ClientActor {
            type Result = ResponseFuture<$ty>;

            fn handle(&mut self, _: $ident, _: &mut Self::Context) -> Self::Result {
                let client = self.client.clone();
                let fut = $handler(client);
                Box::pin(fut)
            }
        }

        impl ClientProxy {
            #[inline]
            pub async fn $by(&self) -> anyhow::Result<$ty> {
                self.call($ident::default()).await
            }
        }
    };
    ($ident:ident($arg0:ty) -> $ty:ty, $by:ident, $handler:expr) => {
        struct $ident($arg0);

        impl Message for $ident {
            type Result = $ty;
        }

        impl Handler<$ident> for ClientActor {
            type Result = ResponseFuture<$ty>;

            fn handle(&mut self, msg: $ident, _: &mut Self::Context) -> Self::Result {
                let client = self.client.clone();
                let fut = $handler(client, msg);
                Box::pin(fut)
            }
        }

        impl ClientProxy {
            #[inline]
            pub async fn $by(&self, val: $arg0) -> anyhow::Result<$ty> {
                self.call($ident(val)).await
            }
        }
    };
}

proxy!(
    IsP2p(NodeId) -> bool,
    is_p2p,
    |client: Client, msg: IsP2p| async move { client.sessions.is_p2p(&msg.0).await }
);
proxy!(
    GetRemoteId(SocketAddr) -> Option<NodeId>,
    remote_id,
    |client: Client, msg: GetRemoteId| async move { client.sessions.remote_id(&msg.0).await }
);
proxy!(
    GetNodeId -> NodeId,
    node_id,
    |client: Client| futures::future::ready(client.node_id())
);
proxy!(
    GetMetrics -> ChannelMetrics,
    metrics,
    |client: Client| futures::future::ready(client.metrics())
);
proxy!(
    GetSockets -> Vec<(SocketDesc, SocketState<ChannelMetrics>)>,
    sockets,
    |client: Client| { futures::future::ready(client.sockets()) }
);
proxy!(
    GetSessions -> Vec<SessionDesc>,
    sessions,
    |client: Client| async move { client.sessions().await }
);
proxy!(
    GetBindAddr -> Option<SocketAddr>,
    bind_addr,
    |client: Client| async move { client.bind_addr().await.ok() }
);
proxy!(
    GetPublicAddr -> Option<SocketAddr>,
    public_addr,
    |client: Client| async move { client.public_addr().await }
);
proxy!(
    ConnectedNodes -> Vec<(NodeId, Option<NodeId>)>,
    connected_nodes,
    |client: Client| async move { client.connected_nodes().await }
);
proxy!(
    PingSessions -> (),
    ping_sessions,
    |client: Client| async move { client.ping_sessions().await }
);
proxy!(
    Shutdown -> anyhow::Result<()>,
    shutdown,
    |mut client: Client| async move { client.shutdown().await }
);
