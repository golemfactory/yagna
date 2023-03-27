use std::sync::{Arc, RwLock};

use ya_core_model::net::local::{BindBroadcastError, BroadcastMessage, SendBroadcastMessage};
use ya_core_model::{identity, NodeId};
use ya_service_api_interfaces::Service;
use ya_service_bus::{Error, RpcEndpoint, RpcMessage};

use crate::config::{Config, NetType};

pub(crate) async fn identities() -> anyhow::Result<(NodeId, Vec<NodeId>)> {
    let ids: Vec<identity::IdentityInfo> = ya_service_bus::typed::service(identity::BUS_ID)
        .send(identity::List::default())
        .await
        .map_err(anyhow::Error::msg)??;

    let mut default_id = None;
    let ids = ids
        .into_iter()
        .map(|id| {
            if id.is_default {
                default_id = Some(id.node_id);
            }
            id.node_id
        })
        .collect::<Vec<NodeId>>();

    let default_id = default_id.ok_or_else(|| anyhow::anyhow!("no default identity"))?;
    Ok((default_id, ids))
}

/// Both Hybrid and Central Net implementation. Only one of them is initialized.
/// TODO: Remove after transitioning to Hybrid Net.
pub struct Net;

lazy_static::lazy_static! {
    pub(crate) static ref NET_TYPE: Arc<RwLock<NetType>> = Default::default();
}

impl Service for Net {
    type Cli = crate::cli::NetCommand;
}

impl Net {
    pub async fn gsb<Context>(ctx: Context) -> anyhow::Result<()> {
        let config = Config::from_env()?;

        {
            (*NET_TYPE.write().unwrap()) = config.net_type;
        }

        match &config.net_type {
            NetType::Central => {
                crate::central::cli::bind_service();
                crate::central::Net::gsb(ctx, config).await
            }
            NetType::Hybrid => {
                crate::hybrid::cli::bind_service();
                crate::hybrid::Net::gsb(ctx, config).await
            }
        }
    }

    pub async fn shutdown() -> anyhow::Result<()> {
        let config = Config::from_env()?;

        {
            (*NET_TYPE.write().unwrap()) = config.net_type;
        }

        match &config.net_type {
            NetType::Central => Ok(()),
            NetType::Hybrid => crate::hybrid::Net::shutdown().await,
        }
    }
}

/// Chooses one of implementations of `broadcast` function
/// for Hybrid Net or for Central Net.
pub async fn broadcast<M, S>(
    caller: S,
    message: M,
) -> Result<
    Result<
        <SendBroadcastMessage<M> as RpcMessage>::Item,
        <SendBroadcastMessage<M> as RpcMessage>::Error,
    >,
    Error,
>
where
    M: BroadcastMessage + Send + Sync + Unpin + 'static,
    S: ToString + 'static,
{
    let net_type = { *NET_TYPE.read().unwrap() };
    match net_type {
        NetType::Central => crate::central::broadcast(caller, message).await,
        NetType::Hybrid => crate::hybrid::broadcast(caller, message).await,
    }
}

/// Chooses one of implementations of `bind_broadcast_with_caller` function
/// for Hybrid Net or for Central Net.
pub async fn bind_broadcast_with_caller<M, T, F>(
    broadcast_address: &str,
    handler: F,
) -> Result<(), BindBroadcastError>
where
    M: BroadcastMessage + Send + Sync + 'static,
    T: std::future::Future<
            Output = Result<
                <SendBroadcastMessage<M> as RpcMessage>::Item,
                <SendBroadcastMessage<M> as RpcMessage>::Error,
            >,
        > + 'static,
    F: FnMut(String, SendBroadcastMessage<M>) -> T + Send + 'static,
{
    let net_type = { *NET_TYPE.read().unwrap() };
    match net_type {
        NetType::Central => {
            crate::central::bind_broadcast_with_caller(broadcast_address, handler).await
        }
        NetType::Hybrid => {
            crate::hybrid::bind_broadcast_with_caller(broadcast_address, handler).await
        }
    }
}
