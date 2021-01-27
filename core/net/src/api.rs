use crate::SUBSCRIPTIONS;
use std::future::Future;
use ya_core_model::net;
use ya_core_model::net::local::{
    BindBroadcastError, BroadcastMessage, SendBroadcastMessage, ToEndpoint,
};
pub use ya_core_model::net::{
    from, net_service, NetApiError, NetDst, NetSrc, RemoteEndpoint, TryRemoteEndpoint,
};
#[cfg(any(feature = "service", test))]
use ya_core_model::NodeId;
use ya_service_bus::{typed as bus, Handle, RpcEndpoint, RpcMessage};

pub async fn bind_broadcast_with_caller<MsgType, Output, F>(
    broadcast_address: &str,
    handler: F,
) -> Result<Handle, BindBroadcastError>
where
    MsgType: BroadcastMessage + Send + Sync + 'static,
    Output: Future<
            Output = Result<
                <SendBroadcastMessage<MsgType> as RpcMessage>::Item,
                <SendBroadcastMessage<MsgType> as RpcMessage>::Error,
            >,
        > + 'static,
    F: FnMut(String, SendBroadcastMessage<MsgType>) -> Output + 'static,
{
    log::debug!("Creating broadcast topic {}.", MsgType::TOPIC);

    // We send Subscribe message to local net, which will create Topic
    // and add broadcast_address to be endpoint, which will be called, when someone
    // will broadcast any Message related to this Topic.
    let subscribe_msg = MsgType::into_subscribe_msg(broadcast_address);
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
        MsgType::TOPIC
    );

    // We created endpoint address above. Now we must add handler, which will
    // handle broadcasts forwarded to this address.
    Ok(bus::bind_with_caller(broadcast_address, handler))
}

#[cfg(any(feature = "service", test))]
pub(crate) fn parse_from_addr(from_addr: &str) -> anyhow::Result<(NodeId, String)> {
    let mut it = from_addr.split("/").fuse();
    if let (Some(""), Some("from"), Some(from_node_id), Some("to"), Some(to_node_id)) =
        (it.next(), it.next(), it.next(), it.next(), it.next())
    {
        to_node_id.parse::<NodeId>()?;
        let prefix = 10 + from_node_id.len();
        let service_id = &from_addr[prefix..];
        if let Some(_) = it.next() {
            return Ok((from_node_id.parse()?, net_service(service_id)));
        }
    }
    anyhow::bail!("invalid net-from destination: {}", from_addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_generated_from_to_service_should_pass() {
        let from_id = "0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df"
            .parse::<NodeId>()
            .unwrap();
        let dst = "0x99402605903da83901151b0871ebeae9296ef66b"
            .parse::<NodeId>()
            .unwrap();

        let remote_service = super::from(from_id).to(dst).service("/public/test/echo");
        let addr = remote_service.addr();
        eprintln!("from/to service address: {}", addr);
        let (parsed_from, parsed_to) = parse_from_addr(addr).unwrap();
        assert_eq!(parsed_from, from_id);
        assert_eq!(
            parsed_to,
            "/net/0x99402605903da83901151b0871ebeae9296ef66b/test/echo"
        );
    }

    #[test]
    fn parse_no_service_should_fail() {
        let out = parse_from_addr("/from/0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df/to/0x99402605903da83901151b0871ebeae9296ef66b");
        assert!(out.is_err())
    }

    #[test]
    fn parse_with_service_should_pass() {
        let out = parse_from_addr("/from/0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df/to/0x99402605903da83901151b0871ebeae9296ef66b/x");
        assert!(out.is_ok())
    }
}
