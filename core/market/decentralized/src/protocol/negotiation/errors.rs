use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum NegotiationApiInitError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ProposalError {
    #[error("Failed to broadcast caused by gsb error: {0}.")]
    GsbError(String),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum AgreementError {
    #[error("Failed to broadcast caused by gsb error: {0}.")]
    GsbError(String),
}

impl From<ya_service_bus::error::Error> for ProposalError {
    fn from(e: ya_service_bus::error::Error) -> Self {
        ProposalError::GsbError(e.to_string())
    }
}

impl From<ya_service_bus::error::Error> for AgreementError {
    fn from(e: ya_service_bus::error::Error) -> Self {
        AgreementError::GsbError(e.to_string())
    }
}
