use actix_http::client::SendRequestError;
use actix_http::error::PayloadError;
use actix_http::ResponseError;
use futures::channel::mpsc::SendError;
use futures::channel::oneshot::Canceled;
use futures::future::Aborted;

#[derive(thiserror::Error, Debug)]
pub enum HttpError {
    #[error("payload error: {0}")]
    PayloadError(PayloadError),
    #[error("send request error: {0}")]
    SendRequestError(String),
    #[error("unspecified")]
    Unspecified,
}

#[derive(thiserror::Error, Debug)]
pub enum ChannelError {
    #[error("cancelled")]
    Cancelled,
}

impl From<Canceled> for ChannelError {
    fn from(_: Canceled) -> Self {
        ChannelError::Cancelled
    }
}

impl From<PayloadError> for HttpError {
    fn from(error: PayloadError) -> Self {
        HttpError::PayloadError(error)
    }
}

impl From<SendRequestError> for HttpError {
    fn from(error: SendRequestError) -> Self {
        HttpError::SendRequestError(error.to_string())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("HTTP error: {0}")]
    HttpError(#[from] HttpError),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Channel error: {0}")]
    ChannelError(#[from] ChannelError),
    #[error("Send error: {0}")]
    SendError(#[from] SendError),
    #[error("GSB error: {0}")]
    Gsb(String),
    #[error("gftp error: {0}")]
    Gftp(#[from] ya_core_model::gftp::Error),
    #[error("URL parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("Invalid url: {0}")]
    InvalidUrlError(String),
    #[error("Unsupported scheme: {0}")]
    UnsupportedSchemeError(String),
    #[error("Unsupported digest: {0}")]
    UnsupportedDigestError(String),
    #[error("Invalid digest: {hash}, expected {expected}")]
    InvalidHashError { hash: String, expected: String },
    #[error("Hex error: {0}")]
    HexError(#[from] hex::FromHexError),
    #[error("Interrupted: {0}")]
    Interrupted(String),
    #[error("Net API error: {0}")]
    NetApiError(#[from] ya_net::NetApiError),
}

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
