use serde::{Deserialize, Serialize};
use thiserror::Error;

use ya_client::model::ErrorMessage;
use ya_core_model::net::local::BindBroadcastError;

use crate::identity::IdentityError;

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryError {
    #[error(transparent)]
    RemoteError(#[from] DiscoveryRemoteError),
    #[error("Failed to broadcast caused by gsb error: {0}.")]
    GsbError(String),
    #[error("Internal error: {0}.")]
    InternalError(String),
    #[error(transparent)]
    Identity(#[from] IdentityError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryRemoteError {
    #[error("Internal error: {0}.")]
    InternalError(String),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryInitError {
    #[error("Failed to bind broadcast `{0}` to gsb. Error: {1}.")]
    BindingGsbFailed(String, String),
    #[error("Failed to subscribe to broadcast `{0}`. Error: {1}.")]
    BroadcastSubscribeFailed(String, String),
}

impl DiscoveryInitError {
    pub(super) fn from_pair(addr: String, e: BindBroadcastError) -> Self {
        match e {
            BindBroadcastError::GsbError(e) => {
                DiscoveryInitError::BindingGsbFailed(addr, e.to_string())
            }
            BindBroadcastError::SubscribeError(e) => {
                DiscoveryInitError::BroadcastSubscribeFailed(addr, e.to_string())
            }
        }
    }
}

impl From<ya_service_bus::error::Error> for DiscoveryError {
    fn from(e: ya_service_bus::error::Error) -> Self {
        DiscoveryError::GsbError(e.to_string())
    }
}

impl From<ErrorMessage> for DiscoveryError {
    fn from(e: ErrorMessage) -> Self {
        DiscoveryError::InternalError(e.to_string())
    }
}
