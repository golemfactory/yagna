use std::cell::RefCell;
use std::rc::Rc;

use bytes::BytesMut;
use futures::SinkExt;
use tokio_util::codec::Encoder;

use ya_core_model::net::local::{
    BindBroadcastError, BroadcastMessage, SendBroadcastMessage, ToEndpoint,
};
use ya_sb_proto::codec::{GsbMessage, GsbMessageEncoder};
use ya_service_bus::{serialization, Error, RpcMessage};

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

    let mut bytes = BytesMut::with_capacity(request.encoded_len());
    let mut encoder = GsbMessageEncoder::default();

    encoder
        .encode(request, &mut bytes)
        .map_err(|e| Error::EncodingProblem(e.to_string()))?;
    sender
        .send(bytes.to_vec())
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
    F: FnMut(String, SendBroadcastMessage<M>) -> T + 'static,
{
    let address_rc: Rc<str> = broadcast_address.into();
    let handler_rc = Rc::new(RefCell::new(handler));

    let subscription = M::into_subscribe_msg(address_rc.to_string());
    let handler = move |caller: String, bytes: &[u8]| {
        match serialization::from_slice::<M>(bytes) {
            Ok(m) => {
                let m = SendBroadcastMessage::new(m);
                let h = handler_rc.clone();
                tokio::task::spawn_local(async move {
                    let _ = (*(h.borrow_mut()))(caller, m).await;
                });
            }
            Err(e) => {
                log::debug!("broadcast msg {} deserialization error: {}", M::TOPIC, e);
            }
        };
    };

    BCAST.with(|b| b.add(subscription));
    BCAST_HANDLERS.with(|h| h.insert(address_rc, Rc::new(RefCell::new(Box::new(handler)))));

    Ok(())
}
