use crate::metrics::error::MetricError;
use serde::Serialize;
use thiserror::Error;
use ya_core_model::activity::RpcMessageError as RpcError;

#[derive(Error, Debug, Serialize)]
pub enum LocalServiceError {
    #[error("Invalid service state: {0}")]
    InvalidState(String),
    #[error("Metric error: {0}")]
    MetricError(
        #[serde(skip)]
        #[from]
        MetricError,
    ),
}

#[derive(Error, Debug, Serialize)]
pub enum SignalError {
    #[error("Unsupported signal: {0}")]
    Unsupported(i32),
}

#[derive(Error, Debug, Serialize)]
pub enum ChannelError {
    #[error("Receive error: {0}")]
    ReceiveError(
        #[serde(skip)]
        #[from]
        crossbeam_channel::RecvError,
    ),
    #[error("Receive error: {0}")]
    TryReceiveError(
        #[serde(skip)]
        #[from]
        crossbeam_channel::TryRecvError,
    ),
    #[error("Receive timeout error: {0}")]
    ReceiveTimeoutError(
        #[serde(skip)]
        #[from]
        crossbeam_channel::RecvTimeoutError,
    ),
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

#[derive(Error, Debug, Serialize)]
pub enum Error {
    #[error("Signal error: {0}")]
    SignalError(#[from] SignalError),
    #[error("IO error: {0}")]
    IoError(
        #[serde(skip)]
        #[from]
        std::io::Error,
    ),
    #[error("Mailbox error: {0}")]
    MailboxError(
        #[serde(skip)]
        #[from]
        actix::prelude::MailboxError,
    ),
    #[error("Channel error: {0}")]
    ChannelError(#[from] ChannelError),
    #[error("Deserialization failed: {0}")]
    JsonError(
        #[serde(skip)]
        #[from]
        serde_json::Error,
    ),
    #[error("Gsb error {0}")]
    GsbError(String),
    #[error("Local service error {0}")]
    LocalServiceError(#[from] LocalServiceError),
    #[error("Remote service error {0}")]
    RemoteServiceError(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
}

impl From<ya_service_bus::Error> for Error {
    fn from(e: ya_service_bus::Error) -> Self {
        Error::GsbError(format!("{:?}", e))
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
            Error::RemoteServiceError(e) => RpcError::Service(e),
            Error::GsbError(e) => RpcError::Service(e),
            Error::UsageLimitExceeded(e) => RpcError::UsageLimitExceeded(e),
        }
    }
}
