use crate::protocol::negotiation::errors::NegotiationApiInitError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NegotiationError {}

#[derive(Error, Debug)]
pub enum NegotiationInitError {
    #[error("Failed to initialize Negotiation interface. Error: {0}.")]
    ApiInitError(#[from] NegotiationApiInitError),
}
