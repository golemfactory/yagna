use anyhow::anyhow;
use async_trait::async_trait;
use golem_base_sdk::{account::TransactionSigner, keccak256, Address};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use ya_client::model::NodeId;

use golem_base_sdk::Signature;
use ya_core_model::identity::{self, IdentityInfo};
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
    #[error("Can't sign data. Error: {0}.")]
    SigningError(String),
}

/// Wraps calls to identity module. It is necessary to mock identity in tests.
#[async_trait::async_trait(?Send)]
pub trait IdentityApi: Send + Sync {
    async fn default_identity(&self) -> Result<NodeId, IdentityError>;
    async fn list(&self) -> Result<Vec<IdentityInfo>, IdentityError>;
    async fn sign(&self, node_id: &NodeId, data: &[u8]) -> Result<Vec<u8>, IdentityError>;

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

    async fn list(&self) -> Result<Vec<IdentityInfo>, IdentityError> {
        Ok(bus::service(identity::BUS_ID)
            .send(identity::List {})
            .await
            .map_err(|e| IdentityError::GsbError(e.to_string()))?
            .map_err(|e| IdentityError::ListError(e.to_string()))?)
    }

    async fn sign(&self, node_id: &NodeId, data: &[u8]) -> Result<Vec<u8>, IdentityError> {
        Ok(bus::service(identity::BUS_ID)
            .send(identity::Sign {
                node_id: node_id.clone(),
                payload: data.to_vec(),
            })
            .await
            .map_err(|e| IdentityError::GsbError(e.to_string()))?
            .map_err(|e| IdentityError::SigningError(e.to_string()))?)
    }
}

#[allow(clippy::new_ret_no_self)]
impl IdentityGSB {
    pub fn new() -> Arc<dyn IdentityApi> {
        Arc::new(IdentityGSB)
    }
}

pub struct YagnaIdSigner {
    pub identity_api: Arc<dyn IdentityApi>,
    pub node_id: NodeId,
}

#[async_trait]
impl TransactionSigner for YagnaIdSigner {
    fn address(&self) -> Address {
        Address::from(&self.node_id.into_array())
    }

    async fn sign(&self, data: &[u8]) -> anyhow::Result<Signature> {
        let hash = keccak256(data);
        let identity_api = self.identity_api.clone();
        let node_id = self.node_id;

        Ok(tokio::task::spawn_local(async move {
            let mut sig_bytes = identity_api.sign(&node_id, hash.as_ref()).await?;
            let v = sig_bytes[0];
            sig_bytes.append(&mut vec![v]);

            anyhow::Ok(
                Signature::from_raw(&sig_bytes[1..])
                    .map_err(|e| anyhow!("Failed to parse signature: {e}"))?,
            )
        })
        .await
        .map_err(|e| anyhow!("Failed to sign data: {e}"))??)
    }
}

impl YagnaIdSigner {
    pub fn new(identity_api: Arc<dyn IdentityApi>, node_id: NodeId) -> Self {
        Self {
            identity_api,
            node_id,
        }
    }
}
