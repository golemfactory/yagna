use crate::agreement;
use crate::metrics::error::MetricError;
use crate::state::StateError;
use thiserror::Error;
use ya_core_model::activity::RpcMessageError as RpcError;
pub use ya_transfer::error::Error as TransferError;

#[derive(Error, Debug)]
pub enum LocalServiceError {
    #[error("State error: {0}")]
    StateError(#[from] StateError),
    #[error("Metric error: {0}")]
    MetricError(#[from] MetricError),
    #[error("Transfer error: {0}")]
    TransferError(#[from] TransferError),
}

#[derive(Error, Debug)]
pub enum SignalError {
    #[error("Unsupported signal: {0}")]
    Unsupported(i32),
}

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("Receive error: {0}")]
    ReceiveError(#[from] crossbeam_channel::RecvError),
    #[error("Receive error: {0}")]
    TryReceiveError(#[from] crossbeam_channel::TryRecvError),
    #[error("Receive timeout error: {0}")]
    ReceiveTimeoutError(#[from] crossbeam_channel::RecvTimeoutError),
    #[error("Send error: {0}")]
    SendError(String),
    #[error("Send error: {0}")]
    TrySendError(String),
    #[error("Send timeout: {0}")]
    SendTimeoutError(String),
}

impl<T> From<crossbeam_channel::SendError<T>> for ChannelError {
    fn from(err: crossbeam_channel::SendError<T>) -> Self {
        ChannelError::SendError(err.to_string())
    }
}

impl<T> From<crossbeam_channel::TrySendError<T>> for ChannelError {
    fn from(err: crossbeam_channel::TrySendError<T>) -> Self {
        ChannelError::TrySendError(err.to_string())
    }
}

impl<T> From<crossbeam_channel::SendTimeoutError<T>> for ChannelError {
    fn from(err: crossbeam_channel::SendTimeoutError<T>) -> Self {
        ChannelError::SendTimeoutError(err.to_string())
    }
}

#[derive(Error, Debug)]
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
    #[error("{0}")]
    CommandError(String),
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
}

impl Error {
    pub fn local<E>(err: E) -> Self
    where
        LocalServiceError: From<E>,
    {
        Error::from(LocalServiceError::from(err))
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
            Error::CommandError(e) => RpcError::Service(e),
            Error::RemoteServiceError(e) => RpcError::Service(e),
            Error::GsbError(e) => RpcError::Service(e),
            Error::UsageLimitExceeded(e) => RpcError::UsageLimitExceeded(e),
        }
    }
}
