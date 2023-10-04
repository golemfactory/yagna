use ya_core_model::net;
use ya_core_model::net::local::{
    BindBroadcastError, BroadcastMessage, SendBroadcastMessage, ToEndpoint,
};
use ya_service_bus::{typed as bus, Error, RpcEndpoint, RpcMessage};

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
    F: FnMut(String, SendBroadcastMessage<M>) -> T + Send + 'static,
{
    log::debug!("Creating broadcast topic {}.", M::TOPIC);

    let address = broadcast_address.to_string();
    let subscription = M::into_subscribe_msg(address.clone());

    log::trace!(
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
