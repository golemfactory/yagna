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
