use ya_core_model::{
    // identity,
    NodeId,
};
/*
use ya_service_bus::{
        typed::service,
        RpcEndpoint
};
*/

pub async fn sign(node_id: NodeId, data: Vec<u8>) -> Result<Vec<u8>, anyhow::Error> {
    /*
    let signature = service(identity::BUS_ID)
        .send(identity::Sign { node_id, data })
        .await
        .map_err(anyhow::Error)?
        .map_err(anyhow::Error)?;
    Ok(signature)
    */
    todo!();
}
