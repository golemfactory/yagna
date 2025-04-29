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
    #[error("GolemBase error: {0}")]
    GolemBaseError(String),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum DiscoveryRemoteError {
    #[error("Internal error: {0}.")]
    InternalError(String),
}

#[derive(Debug, Error)]
pub enum DiscoveryInitError {
    #[error("Failed to bind GSB handler for {0}: {1}")]
    BindingGsbFailed(String, String),

    #[error("Failed to initialize Golem Base client: {0}")]
    GolemBaseInitFailed(String),

    #[error("Builder initialization incomplete: {0}")]
    BuilderIncomplete(String),
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
