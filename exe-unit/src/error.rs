use ya_agreement_utils::agreement;
use ya_core_model::activity::RpcMessageError as RpcError;
pub use ya_transfer::error::Error as TransferError;

use crate::metrics::error::MetricError;
use crate::state::StateError;
use hex::FromHexError;

#[derive(thiserror::Error, Debug)]
pub enum LocalServiceError {
    #[error("State error: {0}")]
    StateError(#[from] StateError),
    #[error("Metric error: {0}")]
    MetricError(#[from] MetricError),
    #[error("Transfer error: {0}")]
    TransferError(#[from] TransferError),
}

#[derive(thiserror::Error, Debug)]
pub enum SignalError {
    #[error("Unsupported signal: {0}")]
    Unsupported(i32),
}

#[derive(thiserror::Error, Debug)]
pub enum ChannelError {
    #[error("Send error: {0}")]
    SendError(String),
    #[error("Send error: {0}")]
    TrySendError(String),
    #[error("Send timeout: {0}")]
    SendTimeoutError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Signal error: {0}")]
    SignalError(#[from] SignalError),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Mailbox error: {0}")]
    MailboxError(#[from] actix::prelude::MailboxError),
    #[error("Channel error: {0}")]
    ChannelError(#[from] ChannelError),
    #[error("Deserialization failed: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Gsb error: {0}")]
    GsbError(String),
    #[error("ExeScript command error: {0}")]
    CommandError(String),
    #[error("ExeScript command exited with code {0}")]
    CommandExitCodeError(i32),
    #[error("Local service error: {0}")]
    LocalServiceError(#[from] LocalServiceError),
    #[error("Remote service error: {0}")]
    RemoteServiceError(String),
    #[error("Runtime error: {0}")]
    RuntimeError(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    #[error("Agreement error: {0}")]
    AgreementError(#[from] agreement::Error),
    #[error("Net error: {0}")]
    Net(#[from] ya_utils_networking::vpn::Error),
    #[error("Net endpoint error: {0}")]
    Endpoint(#[from] ya_utils_networking::socket::EndpointError),
    #[error(transparent)]
    Acl(#[from] crate::acl::Error),
    #[error(transparent)]
    Validation(#[from] crate::manifest::ValidationError),
    #[error("{0}")]
    Other(String),
    #[cfg(feature = "sgx")]
    #[error("Crypto error: {0:?}")]
    Crypto(#[from] secp256k1::Error),
    #[cfg(feature = "sgx")]
    #[error("Attestation error: {0}")]
    Attestation(String),
}

impl Error {
    pub fn local<E>(err: E) -> Self
    where
        LocalServiceError: From<E>,
    {
        Error::from(LocalServiceError::from(err))
    }

    pub fn runtime(err: impl ToString) -> Self {
        Error::RuntimeError(err.to_string())
    }

    pub fn other(err: impl ToString) -> Self {
        Error::Other(err.to_string())
    }
}

impl From<MetricError> for Error {
    fn from(e: MetricError) -> Self {
        Error::from(LocalServiceError::MetricError(e))
    }
}

impl From<StateError> for Error {
    fn from(e: StateError) -> Self {
        Error::from(LocalServiceError::StateError(e))
    }
}

impl From<TransferError> for Error {
    fn from(e: TransferError) -> Self {
        Error::from(LocalServiceError::TransferError(e))
    }
}

impl From<ya_service_bus::Error> for Error {
    fn from(e: ya_service_bus::Error) -> Self {
        Error::GsbError(e.to_string())
    }
}

impl From<FromHexError> for Error {
    fn from(e: FromHexError) -> Self {
        Error::Other(e.to_string())
    }
}

impl From<Error> for RpcError {
    fn from(e: Error) -> Self {
        match e {
            Error::SignalError(e) => RpcError::Activity(e.to_string()),
            Error::IoError(e) => RpcError::Activity(e.to_string()),
            Error::MailboxError(e) => RpcError::Activity(e.to_string()),
            Error::ChannelError(e) => RpcError::Activity(e.to_string()),
            Error::JsonError(e) => RpcError::Activity(e.to_string()),
            Error::LocalServiceError(e) => RpcError::Activity(e.to_string()),
            Error::RuntimeError(e) => RpcError::Activity(e),
            Error::AgreementError(e) => RpcError::Service(e.to_string()),
            Error::CommandError(_) => RpcError::Service(e.to_string()),
            Error::CommandExitCodeError(_) => RpcError::Service(e.to_string()),
            Error::RemoteServiceError(e) => RpcError::Service(e),
            Error::GsbError(e) => RpcError::Service(e),
            Error::UsageLimitExceeded(e) => RpcError::UsageLimitExceeded(e),
            Error::Net(e) => RpcError::Service(e.to_string()),
            Error::Endpoint(e) => RpcError::Service(e.to_string()),
            Error::Acl(e) => RpcError::Forbidden(e.to_string()),
            Error::Validation(e) => RpcError::BadRequest(e.to_string()),
            Error::Other(e) => RpcError::Service(e),
            #[cfg(feature = "sgx")]
            Error::Crypto(e) => RpcError::Service(e.to_string()),
            #[cfg(feature = "sgx")]
            Error::Attestation(e) => RpcError::Service(e.to_string()),
        }
    }
}
