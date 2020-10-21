use futures3::{Future, FutureExt};
use std::pin::Pin;
use tokio::task;
use tokio::task::JoinError;
use ya_client_model::NodeId;
use ya_core_model::identity;
use ya_service_bus::{typed as bus, RpcEndpoint};
use zksync_eth_signer::error::SignerError;

// Copied from core/payment-driver/gnt/utils.rs
pub fn sign_tx(
    node_id: NodeId,
    payload: Vec<u8>
) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, SignerError>> + Send>> {
    let fut = task::spawn_local(async move {
        let signature = bus::service(identity::BUS_ID)
            .send(identity::Sign { node_id, payload })
            .await
            .map_err(|e| SignerError::SigningFailed(format!("{:?}", e)))?
            .map_err(|e| SignerError::SigningFailed(format!("{:?}", e)))?;
        Ok(signature)
    });
    let fut = fut.map(|res| match res {
        Ok(res) => res,
        Err(e) => Err(SignerError::SigningFailed(e.to_string()))
    });
    Box::pin(fut)
}
