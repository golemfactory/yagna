use serde::{Deserialize, Serialize};
use std::sync::Arc;

use ya_client::model::NodeId;

use ya_core_model::identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

#[derive(thiserror::Error, Debug, Serialize, Deserialize)]
pub enum IdentityError {
    #[error("Can't get default identity: {0}.")]
    GetDefaultIdError(String),
    #[error("Can't get identity caused by gsb error: {0}.")]
    GsbError(String),
    #[error("No default identity!!! It shouldn't happen!!")]
    NoDefaultId,
    #[error("Can't list identities. Error: {0}.")]
    ListError(String),
}

/// Wraps calls to identity module. It is necessary to mock identity in tests.
#[async_trait::async_trait(?Send)]
pub trait IdentityApi: Send + Sync {
    async fn default_identity(&self) -> Result<NodeId, IdentityError>;
    async fn list(&self) -> Result<Vec<NodeId>, IdentityError>;
}

pub struct IdentityGSB;

#[async_trait::async_trait(?Send)]
impl IdentityApi for IdentityGSB {
    async fn default_identity(&self) -> Result<NodeId, IdentityError> {
        Ok(bus::service(identity::BUS_ID)
            .send(identity::Get::ByDefault)
            .await
            .map_err(|e| IdentityError::GsbError(e.to_string()))?
            .map_err(|e| IdentityError::GetDefaultIdError(e.to_string()))?
            .ok_or(IdentityError::NoDefaultId)?
            .node_id)
    }

    async fn list(&self) -> Result<Vec<NodeId>, IdentityError> {
        Ok(bus::service(identity::BUS_ID)
            .send(identity::List)
            .await
            .map_err(|e| IdentityError::GsbError(e.to_string()))?
            .map_err(|e| IdentityError::ListError(e.to_string()))?
            .iter()
            .map(|identity_info| identity_info.node_id)
            .collect::<Vec<NodeId>>())
    }
}

#[allow(clippy::new_ret_no_self)]
impl IdentityGSB {
    pub fn new() -> Arc<dyn IdentityApi> {
        Arc::new(IdentityGSB)
    }
}
