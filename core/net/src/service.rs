use ya_core_model::net::net_service;
use ya_core_model::{identity, NodeId};
use ya_service_bus::RpcEndpoint;

pub(crate) async fn identities() -> anyhow::Result<(NodeId, Vec<NodeId>)> {
    let ids: Vec<identity::IdentityInfo> = ya_service_bus::typed::service(identity::BUS_ID)
        .send(identity::List::default())
        .await
        .map_err(anyhow::Error::msg)??;

    let mut default_id = None;
    let ids = ids
        .into_iter()
        .map(|id| {
            if id.is_default {
                default_id = Some(id.node_id);
            }
            id.node_id
        })
        .collect::<Vec<NodeId>>();

    let default_id = default_id.ok_or_else(|| anyhow::anyhow!("no default identity"))?;
    Ok((default_id, ids))
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
    anyhow::bail!("invalid net-from destination: {}", from_addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ya_core_model::net::{from, RemoteEndpoint};

    #[test]
    fn parse_generated_from_to_service_should_pass() {
        let from_id = "0xe93ab94a2095729ad0b7cfa5bfd7d33e1b44d6df"
            .parse::<NodeId>()
            .unwrap();
        let dst = "0x99402605903da83901151b0871ebeae9296ef66b"
            .parse::<NodeId>()
            .unwrap();

        let remote_service = ya_core_model::net::from(from_id)
            .to(dst)
            .service("/public/test/echo");
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
