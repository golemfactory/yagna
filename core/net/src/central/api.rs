use ya_core_model::net;
use ya_core_model::net::local::ToEndpoint;
use ya_core_model::net::local::{BindBroadcastError, BroadcastMessage, SendBroadcastMessage};
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use crate::central::SUBSCRIPTIONS;

pub async fn broadcast<M, S>(
    caller: S,
    message: M,
) -> Result<
    Result<
        <SendBroadcastMessage<M> as RpcMessage>::Item,
        <SendBroadcastMessage<M> as RpcMessage>::Error,
    >,
    ya_service_bus::Error,
>
where
    M: BroadcastMessage + Send + Sync + Unpin + 'static,
    S: ToString + 'static,
{
    // TODO: We shouldn't use send_as. Put identity inside broadcasted message instead.
    bus::service(net::local::BUS_ID)
        .send_as(caller, SendBroadcastMessage::new(message))
        .await
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
    log::debug!("Creating broadcast topic {}.", M::TOPIC);

    // We send Subscribe message to local net, which will create Topic
    // and add broadcast_address to be endpoint, which will be called, when someone
    // will broadcast any Message related to this Topic.
    let subscribe_msg = M::into_subscribe_msg(broadcast_address);
    {
        let mut subscriptions = SUBSCRIPTIONS.lock().unwrap();
        subscriptions.insert(subscribe_msg.clone());
    }

    bus::service(net::local::BUS_ID)
        .send(subscribe_msg)
        .await??;

    log::debug!(
        "Binding handler '{}' for broadcast topic {}.",
        broadcast_address,
        M::TOPIC
    );

    // We created endpoint address above. Now we must add handler, which will
    // handle broadcasts forwarded to this address.
    bus::bind_with_caller(broadcast_address, handler);
    Ok(())
}
