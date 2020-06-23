use anyhow::bail;
use std::future::Future;

use ya_client_model::node_id::{NodeId, ParseError};
use ya_core_model::net;
use ya_core_model::net::local::{
    BindBroadcastError, BroadcastMessage, SendBroadcastMessage, ToEndpoint,
};
use ya_service_bus::{typed as bus, Handle, RpcEndpoint, RpcMessage};

pub(crate) const FROM_BUS_ID: &str = "/from";

#[derive(thiserror::Error, Debug)]
pub enum NetApiError {
    #[error("service bus address should have {} prefix: {0}", net::PUBLIC_PREFIX)]
    PublicPrefixNeeded(String),
    #[error("NodeId parse error: {0}")]
    NodeIdParseError(#[from] ParseError),
}

pub trait TryRemoteEndpoint {
    fn try_service(&self, bus_addr: &str) -> Result<bus::Endpoint, NetApiError>;
}

impl TryRemoteEndpoint for NodeId {
    fn try_service(&self, bus_addr: &str) -> Result<bus::Endpoint, NetApiError> {
        if !bus_addr.starts_with(net::PUBLIC_PREFIX) {
            return Err(NetApiError::PublicPrefixNeeded(bus_addr.into()));
        }
        let exported_part = &bus_addr[net::PUBLIC_PREFIX.len()..];
        let net_bus_addr = format!("{}/{:?}{}", net::BUS_ID, self, exported_part);
        Ok(bus::service(&net_bus_addr))
    }
}

impl TryRemoteEndpoint for &str {
    fn try_service(&self, bus_addr: &str) -> Result<bus::Endpoint, NetApiError> {
        self.parse::<NodeId>()?.try_service(bus_addr)
    }
}

pub struct NetSrc {
    src: NodeId,
}

pub struct NetDst {
    src: NodeId,
    dst: NodeId,
}

pub fn from(src: NodeId) -> NetSrc {
    NetSrc { src }
}

impl NetSrc {
    pub fn to(&self, dst: NodeId) -> NetDst {
        NetDst { src: self.src, dst }
    }
}

#[inline]
pub(crate) fn net_service(service: impl ToString) -> String {
    format!("{}/{}", net::BUS_ID, service.to_string())
}

fn extract_exported_part(local_service_addr: &str) -> &str {
    assert!(local_service_addr.starts_with(net::PUBLIC_PREFIX));
    &local_service_addr[net::PUBLIC_PREFIX.len()..]
}

pub trait RemoteEndpoint {
    fn service(&self, bus_addr: &str) -> bus::Endpoint;
}

impl RemoteEndpoint for NodeId {
    fn service(&self, bus_addr: &str) -> bus::Endpoint {
        bus::service(format!(
            "{}/{}",
            net_service(self),
            extract_exported_part(bus_addr)
        ))
    }
}

impl RemoteEndpoint for NetDst {
    fn service(&self, bus_addr: &str) -> bus::Endpoint {
        bus::service(format!(
            "/from/{}/to/{}{}",
            self.src,
            self.dst,
            extract_exported_part(bus_addr)
        ))
    }
}

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
    bail!("invalid net-from destination: {}", from_addr)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_try_service_on_public() {
        "0xbabe000000000000000000000000000000000000"
            .try_service("/public/x")
            .unwrap();
    }

    #[test]
    fn err_try_service_on_non_public() {
        let result = "0xbabe000000000000000000000000000000000000".try_service("/zima/x");
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "service bus address should have /public prefix: /zima/x".to_string()
        )
    }

    #[test]
    fn err_try_service_on_non_node_id() {
        assert!("lato".try_service("/zima/x").is_err());
    }

    #[test]
    fn ok_try_service_on_node_id() {
        let node_id: NodeId = "0xbabe000000000000000000000000000000000000"
            .parse()
            .unwrap();
        node_id.try_service("/public/x").unwrap();
    }

    #[test]
    fn err_try_service_on_node_id_and_non_public() {
        let node_id: NodeId = "0xbabe000000000000000000000000000000000000"
            .parse()
            .unwrap();
        assert!(node_id.try_service("/zima/x").is_err());
    }

    #[test]
    fn parse_generated_from_to_service_should_pass() {
        let from_id = "0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df"
            .parse::<NodeId>()
            .unwrap();
        let dst = "0x99402605903da83901151b0871ebeae9296ef66b"
            .parse::<NodeId>()
            .unwrap();

        let remote_service = crate::from(from_id).to(dst).service("/public/test/echo");
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

    #[test]
    fn ok_net_node_id() {
        let node_id: NodeId = "0xbabe000000000000000000000000000000000000"
            .parse()
            .unwrap();
        assert_eq!(
            net_service(&node_id),
            "/net/0xbabe000000000000000000000000000000000000".to_string()
        );
    }
}
