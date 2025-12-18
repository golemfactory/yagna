use serde::{Deserialize, Serialize};
use std::sync::Arc;

use ya_client::model::NodeId;
use ya_core_model::{
    bus::GsbBindPoints,
    identity::{self, IdentityInfo},
    market,
};
use ya_service_bus::RpcEndpoint;

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
    #[error("Can't sign data. Error: {0}.")]
    SigningError(String),
}

/// Wraps calls to identity module. It is necessary to mock identity in tests.
#[async_trait::async_trait(?Send)]
pub trait IdentityApi: Send + Sync {
    async fn default_identity(&self) -> Result<NodeId, IdentityError>;
    async fn list(&self) -> Result<Vec<IdentityInfo>, IdentityError>;
    async fn sign(&self, node_id: &NodeId, data: &[u8]) -> Result<Vec<u8>, IdentityError>;
    async fn subscribe_to_events(&self, endpoint: &str) -> Result<(), IdentityError>;

    async fn list_ids(&self) -> Result<Vec<NodeId>, IdentityError> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .map(|info| info.node_id)
            .collect::<Vec<NodeId>>())
    }

    async fn list_active_ids(&self) -> Result<Vec<NodeId>, IdentityError> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .filter_map(|info| match info.is_locked || info.deleted {
                true => None,
                false => Some(info.node_id),
            })
            .collect::<Vec<NodeId>>())
    }

    async fn fund(&self, wallet: NodeId) -> Result<(), IdentityError>;
}

#[derive(Clone)]
pub struct IdentityGSB {
    gsb: GsbBindPoints,
}

#[async_trait::async_trait(?Send)]
impl IdentityApi for IdentityGSB {
    async fn default_identity(&self) -> Result<NodeId, IdentityError> {
        Ok(self
            .gsb
            .local()
            .send(identity::Get::ByDefault)
            .await
            .map_err(|e| IdentityError::GsbError(e.to_string()))?
            .map_err(|e| IdentityError::GetDefaultIdError(e.to_string()))?
            .ok_or(IdentityError::NoDefaultId)?
            .node_id)
    }

    async fn list(&self) -> Result<Vec<IdentityInfo>, IdentityError> {
        Ok(self
            .gsb
            .local()
            .send(identity::List {})
            .await
            .map_err(|e| IdentityError::GsbError(e.to_string()))?
            .map_err(|e| IdentityError::ListError(e.to_string()))?)
    }

    async fn sign(&self, node_id: &NodeId, data: &[u8]) -> Result<Vec<u8>, IdentityError> {
        Ok(self
            .gsb
            .local()
            .send(identity::Sign {
                node_id: *node_id,
                payload: data.to_vec(),
            })
            .await
            .map_err(|e| IdentityError::GsbError(e.to_string()))?
            .map_err(|e| IdentityError::SigningError(e.to_string()))?)
    }

    async fn subscribe_to_events(&self, endpoint: &str) -> Result<(), IdentityError> {
        self.gsb
            .local()
            .send(identity::Subscribe {
                endpoint: endpoint.to_string(),
            })
            .await
            .map_err(|e| IdentityError::GsbError(e.to_string()))?
            .map(|_| ())
            .map_err(|e| IdentityError::GsbError(e.to_string()))
    }

    async fn fund(&self, wallet: NodeId) -> Result<(), IdentityError> {
        self.gsb
            .local()
            .send(market::FundGolemBase {
                wallet: Some(wallet),
            })
            .await
            .map_err(|e| IdentityError::GsbError(e.to_string()))?
            .map_err(|e| IdentityError::GsbError(e.to_string()))?;
        Ok(())
    }
}

#[allow(clippy::new_ret_no_self)]
impl IdentityGSB {
    pub fn new(gsb: GsbBindPoints) -> Arc<dyn IdentityApi> {
        Arc::new(IdentityGSB {
            gsb: gsb.service(identity::BUS_SERVICE_NAME),
        })
    }
}
