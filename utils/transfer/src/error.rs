use actix_http::client::SendRequestError;
use actix_http::error::PayloadError;
use actix_http::ResponseError;
use futures::channel::mpsc::SendError;
use futures::future::Aborted;

#[derive(thiserror::Error, Debug)]
pub enum HttpError {
    #[error("payload error: {0}")]
    PayloadError(PayloadError),
    #[error("send request error: {0}")]
    SendRequestError(SendRequestError),
    #[error("unspecified")]
    Unspecified,
}

unsafe impl Send for HttpError {}

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

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("HTTP error: {0}")]
    HttpError(#[from] HttpError),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Send error: {0}")]
    SendError(#[from] SendError),
    #[error("URL parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("Invalid url: {0}")]
    InvalidUrlError(String),
    #[error("Unsupported digest: {0}")]
    UnsupportedDigestError(String),
    #[error("Invalid digest: {hash}, expected {expected}")]
    InvalidHashError { hash: String, expected: String },
    #[error("Hex error: {0}")]
    HexError(#[from] hex::FromHexError),
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
