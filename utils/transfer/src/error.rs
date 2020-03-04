use actix_http::client::SendRequestError;
use actix_http::error::PayloadError;
use actix_http::ResponseError;
use futures::channel::mpsc::SendError;
use futures::channel::oneshot::Canceled;
use futures::future::Aborted;
use serde::Serialize;

#[derive(thiserror::Error, Debug, Serialize)]
pub enum HttpError {
    #[error("payload error: {0}")]
    PayloadError(#[serde(skip)] PayloadError),
    #[error("send request error: {0}")]
    SendRequestError(#[serde(skip)] SendRequestError),
    #[error("unspecified")]
    Unspecified,
}

unsafe impl Send for HttpError {}

#[derive(thiserror::Error, Debug, Serialize)]
pub enum ChannelError {
    #[error("cancelled")]
    Cancelled,
}

impl From<Canceled> for ChannelError {
    fn from(_: Canceled) -> Self {
        ChannelError::Cancelled
    }
}

unsafe impl Send for ChannelError {}

impl From<PayloadError> for HttpError {
    fn from(error: PayloadError) -> Self {
        HttpError::PayloadError(error)
    }
}

impl From<SendRequestError> for HttpError {
    fn from(error: SendRequestError) -> Self {
        HttpError::SendRequestError(error)
    }
}

#[derive(thiserror::Error, Debug, Serialize)]
pub enum Error {
    #[error("HTTP error: {0}")]
    HttpError(#[from] HttpError),
    #[error("IO error: {0}")]
    IoError(
        #[from]
        #[serde(skip)]
        std::io::Error,
    ),
    #[error("Channel error: {0}")]
    ChannelError(#[from] ChannelError),
    #[error("Send error: {0}")]
    SendError(
        #[from]
        #[serde(skip)]
        SendError,
    ),
    #[error("GSB error: {0}")]
    Gsb(String),
    #[error("gftp error: {0}")]
    Gftp(#[from] ya_core_model::gftp::Error),
    #[error("URL parse error: {0}")]
    UrlParseError(
        #[from]
        #[serde(skip)]
        url::ParseError,
    ),
    #[error("Invalid url: {0}")]
    InvalidUrlError(String),
    #[error("Unsupported scheme: {0}")]
    UnsupportedSchemeError(String),
    #[error("Unsupported digest: {0}")]
    UnsupportedDigestError(String),
    #[error("Invalid digest: {hash}, expected {expected}")]
    InvalidHashError { hash: String, expected: String },
    #[error("Hex error: {0}")]
    HexError(
        #[from]
        #[serde(skip)]
        hex::FromHexError,
    ),
    #[error("Interrupted: {0}")]
    Interrupted(String),
}

unsafe impl Send for Error {}

impl ResponseError for Error {}

impl From<Aborted> for Error {
    fn from(_: Aborted) -> Self {
        Error::Interrupted("Action aborted".to_owned())
    }
}

impl From<PayloadError> for Error {
    fn from(error: PayloadError) -> Self {
        Error::HttpError(HttpError::from(error))
    }
}

impl From<SendRequestError> for Error {
    fn from(error: SendRequestError) -> Self {
        Error::HttpError(HttpError::from(error))
    }
}

impl From<ya_service_bus::error::Error> for Error {
    fn from(e: ya_service_bus::error::Error) -> Self {
        log::debug!("ya_service_bus::error::Error: {:?}", e);
        Error::Gsb(e.to_string())
    }
}
