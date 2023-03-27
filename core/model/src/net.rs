use ya_client_model::node_id::ParseError;
use ya_client_model::NodeId;
use ya_service_bus::{typed as bus, RpcMessage};

use serde::{Deserialize, Serialize};
use ya_service_bus::typed::Endpoint;

pub const BUS_ID: &str = "/net";
pub const BUS_ID_UDP: &str = "/udp/net";
pub const BUS_ID_TRANSFER: &str = "/transfer/net";

// TODO: replace with dedicated endpoint/service descriptor with enum for visibility
pub const PUBLIC_PREFIX: &str = "/public";

pub const DIAGNOSTIC: &str = "/public/diagnostic/net";

///
///
pub mod local {
    use std::net::SocketAddr;
    use std::time::Duration;

    use serde::de::DeserializeOwned;
    use serde::{Deserialize, Serialize};

    use ya_client_model::NodeId;
    use ya_service_bus::RpcMessage;

    pub const BUS_ID: &str = "/local/net";

    pub trait BroadcastMessage: Serialize + DeserializeOwned {
        const TOPIC: &'static str;
    }

    #[derive(Serialize, Deserialize)]
    pub struct SendBroadcastStub {
        pub id: Option<String>,
        pub topic: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct SendBroadcastMessage<M> {
        id: Option<String>,
        topic: String,
        body: M,
    }

    impl<M: BroadcastMessage> SendBroadcastMessage<M> {
        pub fn new(body: M) -> Self {
            let id = None;
            let topic = M::TOPIC.to_owned();
            Self { id, topic, body }
        }

        pub fn body(&self) -> &M {
            &self.body
        }
    }

    impl<M> SendBroadcastMessage<M> {
        pub fn topic(&self) -> &str {
            self.topic.as_ref()
        }

        pub fn set_id(&mut self, id: String) {
            self.id = Some(id)
        }
    }

    impl<M: Send + Sync + Serialize + DeserializeOwned + 'static> RpcMessage
        for SendBroadcastMessage<M>
    {
        const ID: &'static str = "SendBroadcastMessage";
        type Item = ();
        type Error = ();
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
    #[serde(rename_all = "camelCase")]
    pub struct Subscribe {
        topic: String,
        endpoint: String,
    }

    impl Subscribe {
        pub fn topic(&self) -> &str {
            self.topic.as_ref()
        }

        pub fn endpoint(&self) -> &str {
            self.endpoint.as_ref()
        }
    }

    pub trait ToEndpoint<M: BroadcastMessage> {
        fn into_subscribe_msg(endpoint: impl Into<String>) -> Subscribe;
    }

    impl<M: BroadcastMessage> ToEndpoint<M> for M {
        fn into_subscribe_msg(endpoint: impl Into<String>) -> Subscribe {
            let topic = M::TOPIC.to_owned();
            let endpoint = endpoint.into();
            Subscribe { topic, endpoint }
        }
    }

    impl RpcMessage for Subscribe {
        const ID: &'static str = "Subscribe";
        type Item = u64;
        type Error = SubscribeError;
    }

    #[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub enum SubscribeError {
        #[error("{0}")]
        RuntimeException(String),
    }

    #[derive(thiserror::Error, Debug)]
    pub enum BindBroadcastError {
        #[error(transparent)]
        SubscribeError(#[from] SubscribeError),
        #[error(transparent)]
        GsbError(#[from] ya_service_bus::error::Error),
    }

    #[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
    pub enum StatusError {
        #[error("{0}")]
        RuntimeException(String),
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
    #[serde(rename_all = "camelCase")]
    pub struct Status {}

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct StatusMetrics {
        pub tx_total: usize,
        pub tx_current: f32,
        pub tx_avg: f32,
        pub rx_total: usize,
        pub rx_current: f32,
        pub rx_avg: f32,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct StatusResponse {
        pub node_id: NodeId,
        pub listen_address: Option<SocketAddr>,
        pub public_address: Option<SocketAddr>,
        pub sessions: usize,
        pub metrics: StatusMetrics,
    }

    impl RpcMessage for Status {
        const ID: &'static str = "Status";
        type Item = StatusResponse;
        type Error = StatusError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
    #[serde(rename_all = "camelCase")]
    pub struct Sessions {}

    impl RpcMessage for Sessions {
        const ID: &'static str = "Sessions";
        type Item = Vec<SessionResponse>;
        type Error = StatusError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SessionResponse {
        pub node_id: Option<NodeId>,
        pub id: String,
        pub session_type: String,
        pub remote_address: SocketAddr,
        pub seen: Duration,
        pub duration: Duration,
        pub ping: Duration,
        pub metrics: StatusMetrics,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
    #[serde(rename_all = "camelCase")]
    pub struct Sockets {}

    impl RpcMessage for Sockets {
        const ID: &'static str = "Sockets";
        type Item = Vec<SocketResponse>;
        type Error = StatusError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SocketResponse {
        pub protocol: String,
        pub state: String,
        pub local_port: String,
        pub remote_addr: String,
        pub remote_port: String,
        pub metrics: StatusMetrics,
    }

    #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
    #[serde(rename_all = "camelCase")]
    pub struct FindNode {
        pub node_id: String,
    }

    impl RpcMessage for FindNode {
        const ID: &'static str = "FindNode";
        type Item = FindNodeResponse;
        type Error = StatusError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FindNodeResponse {
        pub identities: Vec<NodeId>,
        pub endpoints: Vec<SocketAddr>,
        pub seen: u32,
        pub slot: u32,
        pub encryption: Vec<String>,
    }

    /// Measures time between sending GSB message and getting response.
    /// This is different from session ping, because it takes into account
    /// Virtual TCP overhead. Moreover we can measure ping between Nodes
    /// using `ya-relay-server` for communication.
    #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
    #[serde(rename_all = "camelCase")]
    pub struct GsbPing {}

    impl RpcMessage for GsbPing {
        const ID: &'static str = "GsbPing";
        type Item = Vec<GsbPingResponse>;
        type Error = StatusError;
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GsbPingResponse {
        pub node_id: NodeId,
        pub node_alias: Option<NodeId>,
        pub tcp_ping: Duration,
        pub udp_ping: Duration,
        pub is_p2p: bool,
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct NewNeighbour;

    impl BroadcastMessage for NewNeighbour {
        const TOPIC: &'static str = "new-neighbour";
    }
}

/// For documentation check local::GsbPing
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct GsbRemotePing {}

impl RpcMessage for GsbRemotePing {
    const ID: &'static str = "GsbRemotePing";
    type Item = GsbRemotePing;
    type Error = GenericNetError;
}

#[derive(thiserror::Error, Debug, Serialize, Deserialize)]
#[error("{0}")]
pub struct GenericNetError(pub String);

#[derive(thiserror::Error, Debug)]
pub enum NetApiError {
    #[error("service bus address should have {} prefix: {0}", PUBLIC_PREFIX)]
    PublicPrefixNeeded(String),
    #[error("NodeId parse error: {0}")]
    NodeIdParseError(#[from] ParseError),
}

pub trait TryRemoteEndpoint {
    fn try_service(&self, bus_addr: &str) -> Result<bus::Endpoint, NetApiError>;
}

impl TryRemoteEndpoint for NodeId {
    fn try_service(&self, bus_addr: &str) -> Result<bus::Endpoint, NetApiError> {
        if !bus_addr.starts_with(PUBLIC_PREFIX) {
            return Err(NetApiError::PublicPrefixNeeded(bus_addr.into()));
        }
        let exported_part = &bus_addr[PUBLIC_PREFIX.len()..];
        let net_bus_addr = format!("{}/{:?}{}", BUS_ID, self, exported_part);
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
pub fn net_service(service: impl ToString) -> String {
    format!("{}/{}", BUS_ID, service.to_string())
}

#[inline]
pub fn net_service_udp(service: impl ToString) -> String {
    format!("{}/{}", BUS_ID_UDP, service.to_string())
}

#[inline]
pub fn net_transfer_service(service: impl ToString) -> String {
    format!("{}/{}", BUS_ID_TRANSFER, service.to_string())
}

fn extract_exported_part(local_service_addr: &str) -> &str {
    assert!(local_service_addr.starts_with(PUBLIC_PREFIX));
    &local_service_addr[PUBLIC_PREFIX.len()..]
}

pub trait RemoteEndpoint {
    fn service(&self, bus_addr: &str) -> bus::Endpoint;
    fn service_udp(&self, bus_addr: &str) -> bus::Endpoint;
    fn service_transfer(&self, bus_addr: &str) -> bus::Endpoint;
}

impl RemoteEndpoint for NodeId {
    fn service(&self, bus_addr: &str) -> bus::Endpoint {
        bus::service(format!(
            "{}{}",
            net_service(self),
            extract_exported_part(bus_addr)
        ))
    }

    fn service_udp(&self, bus_addr: &str) -> Endpoint {
        bus::service(format!(
            "{}{}",
            net_service_udp(self),
            extract_exported_part(bus_addr)
        ))
    }

    fn service_transfer(&self, bus_addr: &str) -> Endpoint {
        bus::service(format!(
            "{}{}",
            net_transfer_service(self),
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

    fn service_udp(&self, bus_addr: &str) -> Endpoint {
        bus::service(format!(
            "/udp/from/{}/to/{}{}",
            self.src,
            self.dst,
            extract_exported_part(bus_addr)
        ))
    }

    fn service_transfer(&self, bus_addr: &str) -> Endpoint {
        bus::service(format!(
            "/transfer/from/{}/to/{}{}",
            self.src,
            self.dst,
            extract_exported_part(bus_addr)
        ))
    }
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
    fn ok_net_node_id() {
        let node_id: NodeId = "0xbabe000000000000000000000000000000000000"
            .parse()
            .unwrap();
        assert_eq!(
            net_service(&node_id),
            "/net/0xbabe000000000000000000000000000000000000".to_string()
        );
    }

    #[test]
    fn test_transfer_service() {
        let node_id: NodeId = "0xbabe000000000000000000000000000000000000"
            .parse()
            .unwrap();
        assert_eq!(
            node_id.service_transfer("/public/zima/x").addr(),
            "/transfer/net/0xbabe000000000000000000000000000000000000/zima/x"
        );
    }
}
