use ya_client_model::node_id::ParseError;
use ya_client_model::NodeId;
use ya_service_bus::typed as bus;

pub const BUS_ID: &str = "/net";
pub const BUS_ID_UDP: &str = "/udp/net";

// TODO: replace with dedicated endpoint/service descriptor with enum for visibility
pub const PUBLIC_PREFIX: &str = "/public";

///
///
pub mod local {
    use serde::de::DeserializeOwned;
    use serde::{Deserialize, Serialize};
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
}

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

fn extract_exported_part(local_service_addr: &str) -> &str {
    assert!(local_service_addr.starts_with(PUBLIC_PREFIX));
    &local_service_addr[PUBLIC_PREFIX.len()..]
}

pub trait RemoteEndpoint {
    fn service(&self, bus_addr: &str) -> bus::Endpoint;
}

impl RemoteEndpoint for NodeId {
    fn service(&self, bus_addr: &str) -> bus::Endpoint {
        bus::service(format!(
            "{}{}",
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
}
