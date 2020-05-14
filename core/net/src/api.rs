use ya_client_model::node_id::{NodeId, ParseError};
use ya_core_model::net;
use ya_service_bus::typed as bus;

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

pub fn net_node_id(node_id: &NodeId) -> String {
    format!("{}/{:?}", net::BUS_ID, node_id)
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
            net_node_id(&node_id),
            "/net/0xbabe000000000000000000000000000000000000".to_string()
        );
    }
}
