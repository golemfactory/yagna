//! Error definitions and mappings
use backtrace::Backtrace;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("AWC error requesting {url}: {e}, ")]
    SendRequestError {
        e: awc::error::SendRequestError,
        url: String,
    },
    #[error("AWC payload error: {e}, {b}")]
    PayloadError {
        e: awc::error::PayloadError,
        b: String,
    },
    #[error("AWC JSON payload error: {e}, {b}")]
    JsonPayloadError {
        e: awc::error::JsonPayloadError,
        b: String,
    },
    #[error("HTTP status code: {0}")]
    HttpStatusCode(awc::http::StatusCode),
    #[error("serde JSON error: {0}")]
    SerdeJsonError(serde_json::Error),
    #[error("invalid address: {0}")]
    InvalidAddress(std::convert::Infallible),
    #[error("invalid header: {0}")]
    InvalidHeadeName(#[from] awc::http::header::InvalidHeaderName),
    #[error("invalid header: {0}")]
    InvalidHeaderValue(#[from] awc::http::header::InvalidHeaderValue),
    #[error("invalid UTF8 string: {0}")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
    #[error("Url parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
}

impl From<awc::error::SendRequestError> for Error {
    fn from(e: awc::error::SendRequestError) -> Self {
        Error::SendRequestError {
            e,
            url: format!("{:#?}", Backtrace::new()),
        }
    }
}

impl From<awc::error::PayloadError> for Error {
    fn from(e: awc::error::PayloadError) -> Self {
        Error::PayloadError {
            e,
            b: format!("{:#?}", Backtrace::new()),
        }
    }
}

impl From<awc::error::JsonPayloadError> for Error {
    fn from(e: awc::error::JsonPayloadError) -> Self {
        Error::JsonPayloadError {
            e,
            b: format!("{:#?}", Backtrace::new()),
        }
    }
}
