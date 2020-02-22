use super::PUBLIC_PREFIX;
use ya_core_model::ethaddr::NodeId;
use ya_service_bus::typed as bus;
use ya_service_bus::typed::Endpoint;

pub trait RemoteEndpoint {
    fn service(&self, addr: &str) -> bus::Endpoint;
}

impl RemoteEndpoint for NodeId {
    fn service(&self, addr: &str) -> Endpoint {
        assert!(addr.starts_with(PUBLIC_PREFIX));
        let exported_part = &addr[PUBLIC_PREFIX.len()..];
        let addr = format!("/net/{:?}/{}", self, exported_part);
        bus::service(&addr)
    }
}
