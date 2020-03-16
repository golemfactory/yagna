use ya_core_model::{ethaddr::NodeId, net};
use ya_service_bus::{typed as bus, PRIVATE_PREFIX, PUBLIC_PREFIX};

pub trait RemoteEndpoint {
    fn service(&self, addr: &str) -> bus::Endpoint;
}

impl RemoteEndpoint for NodeId {
    fn service(&self, addr: &str) -> bus::Endpoint {
        assert!(addr.starts_with(PUBLIC_PREFIX));
        let exported_part = &addr[PUBLIC_PREFIX.len()..];
        let addr = format!("/net/{:?}/{}", self, exported_part);
        bus::service(&addr)
    }
}

pub fn remote_service(node_id: impl ToString, service_id: &str) -> String {
    // FIXME: use NodeId
    assert!(service_id.starts_with("/"));
    format!(
        "{}{}/{}{}",
        PRIVATE_PREFIX,
        net::SERVICE_ID,
        node_id.to_string(),
        service_id
    )
}

pub fn net_node_id(node_id: impl ToString) -> String {
    format!("{}/{}", net::SERVICE_ID, node_id.to_string())
}

/// caller might start with net prefix, but not have to
#[inline(always)]
pub fn authorize_caller(caller: impl ToString, authorized: &String) -> bool {
    // FIXME: impl a proper caller struct / parser
    let net_prefix = format!("{}/", net::SERVICE_ID);
    let caller = caller.to_string().replacen(&net_prefix, "", 1);
    log::debug!("checking caller: {} vs authorized: {}", caller, authorized);
    &caller == authorized
}

#[cfg(test)]
mod tests {
    use crate::remote_service;

    #[test]
    fn remote_service_empty() {
        assert_eq!(remote_service("", "/"), "/private/net//")
    }

    #[test]
    fn remote_service_node_only() {
        assert_eq!(remote_service("node", "/"), "/private/net/node/")
    }

    #[test]
    fn remote_service_service_only() {
        assert_eq!(remote_service("", "/srv"), "/private/net//srv")
    }

    #[test]
    fn remote_service_both() {
        assert_eq!(remote_service("node", "/srv"), "/private/net/node/srv")
    }
}
