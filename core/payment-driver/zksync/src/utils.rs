use futures3::{Future, FutureExt};
use std::pin::Pin;
use ya_client_model::NodeId;
use ya_core_model::identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

// Copied from core/payment-driver/gnt/utils.rs
pub fn get_sign_tx(node_id: NodeId) -> impl Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>> + Send>> {
    move |payload| {
        let fut = bus::service(identity::BUS_ID)
            .send(identity::Sign { node_id, payload })
            .map(|x| x.unwrap().unwrap());
        Box::pin(fut)
    }
}
