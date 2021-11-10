use std::sync::{Arc, Mutex};

use futures::SinkExt;

use ya_core_model::net::local::{
    BindBroadcastError, BroadcastMessage, SendBroadcastMessage, ToEndpoint,
};
use ya_sb_proto::codec::GsbMessage;
use ya_service_bus::{serialization, Error, RpcMessage};

use crate::hybrid::codec::encode_message;
use crate::hybrid::service::{BCAST, BCAST_HANDLERS, BCAST_SENDER};

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
    let mut sender = BCAST_SENDER
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| Error::Closed("network not initialized".to_string()))?;

    let request = GsbMessage::BroadcastRequest(ya_sb_proto::BroadcastRequest {
        data: serialization::to_vec(&message)?,
        caller: caller.to_string(),
        topic: M::TOPIC.to_owned(),
    });

    let bytes = encode_message(request).map_err(|e| Error::EncodingProblem(e.to_string()))?;
    sender
        .send(bytes)
        .await
        .map_err(|_| Error::Closed("broadcast channel is closed".to_string()))?;

    Ok(Ok(()))
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
    let address = broadcast_address.to_string();
    let handler_rc = Arc::new(Mutex::new(handler));

    let subscription = M::into_subscribe_msg(address.clone());

    log::trace!(
        "binding broadcast handler for topic: {}",
        subscription.topic()
    );

    let handler = move |caller: String, bytes: &[u8]| {
        match serialization::from_slice::<M>(bytes) {
            Ok(m) => {
                let m = SendBroadcastMessage::new(m);
                let handler = handler_rc.clone();
                tokio::task::spawn_local(async move {
                    let mut h = handler.lock().unwrap();
                    let _ = (*(h))(caller, m).await;
                });
            }
            Err(e) => {
                log::debug!("broadcast msg {} deserialization error: {}", M::TOPIC, e);
            }
        };
    };

    BCAST.add(subscription);
    BCAST_HANDLERS
        .lock()
        .unwrap()
        .insert(address, Arc::new(Mutex::new(Box::new(handler))));

    Ok(())
}
