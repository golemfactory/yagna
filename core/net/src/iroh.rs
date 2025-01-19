use ya_core_model::net::local::{BindBroadcastError, BroadcastMessage, SendBroadcastMessage, ToEndpoint};
use std::sync::Arc;
use actix_web::Scope;
use futures::{future, stream};
use ya_service_bus::{Error, RpcEndpoint, RpcMessage};
use crate::Config;
use net_iroh::NetClient;
use ya_service_bus::untyped::{Fn4HandlerExt, Fn4StreamHandlerExt};
use ya_service_bus::typed as bus;
use ya_core_model::net;

mod local_service;
mod cli;
mod bridge;
mod crypto;
mod rpc;


pub struct IRohNet;

pub async fn gsb<Context>(_: Context, config: Config) -> anyhow::Result<()> {
    use ya_service_bus::{untyped as gsb, error::Error as GsbError, ResponseChunk};

    let (default_id, ids) = crate::service::identities().await?;
    let crypto = self::crypto::IdentityCryptoProvider::new(default_id);
    let client = NetClient::builder().bind_url(config.bind_url).crypto_provider(crypto).start().await?;

    // /net/{id}/{service} -> /public/{service}
    // /transfer/net/{dst}/service
    // /udp/net/{dst}/service

    // /from/{src}/to/{dst}/{service} -> /public/{service}
    // /udp/from/{src}/to/{dst}/{service} -> /public/{service}
    // /transfer/from/{src}/to/{dst}/{service} -> /public/{service}


    // -> Future<Result<Vec<u8>, GsbError>
    let rpc = move |caller: &str, addr: &str, msg: &[u8], no_reply: bool| {

        future::ok(Vec::new())
    };

    let rpc_stream = move |caller: &str, addr: &str, msg: &[u8], no_reply: bool| {

        stream::once(future::ok(ResponseChunk::Full(Vec::new())))
    };

    let _ = gsb::subscribe("/net", rpc.into_handler(), rpc_stream.into_stream_handler());

    Ok(())
}

pub fn scope() -> Scope {
    todo!()
}


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
    F: FnMut(String, SendBroadcastMessage<M>) -> T + 'static,
{
    let address = broadcast_address.to_string();
    let subscription = M::into_subscribe_msg(address.clone());

    log::debug!(
        "Binding broadcast handler for topic: {}",
        subscription.topic()
    );

    bus::service(net::local::BUS_ID)
        .send(subscription)
        .await??;

    log::debug!(
        "Binding handler '{broadcast_address}' for broadcast topic {}.",
        M::TOPIC
    );

    // We created endpoint address above. Now we must add handler, which will
    // handle broadcasts forwarded to this address.
    bus::bind_with_caller(broadcast_address, handler);
    Ok(())
}