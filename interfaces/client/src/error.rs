//! Error definitions and mappings
use backtrace::Backtrace as Trace; // needed b/c of thiserror magic
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("AWC error requesting {url}: {e}")]
    SendRequestError {
        e: awc::error::SendRequestError,
        url: String,
        bt: Trace,
    },
    #[error("AWC payload error: {e}")]
    PayloadError {
        e: awc::error::PayloadError,
        bt: Trace,
    },
    #[error("AWC JSON payload error: {e}")]
    JsonPayloadError {
        e: awc::error::JsonPayloadError,
        bt: Trace,
    },
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::error::Error),
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
            url: "".into(),
            bt: Trace::new(),
        }
    }
}

impl From<(awc::error::SendRequestError, String)> for Error {
    fn from(pair: (awc::error::SendRequestError, String)) -> Self {
        Error::SendRequestError {
            e: pair.0,
            url: pair.1,
            bt: Trace::new(),
        }
    }
}

impl From<awc::error::PayloadError> for Error {
    fn from(e: awc::error::PayloadError) -> Self {
        Error::PayloadError {
            e,
            bt: Trace::new(),
        }
    }
}

impl From<awc::error::JsonPayloadError> for Error {
    fn from(e: awc::error::JsonPayloadError) -> Self {
        Error::JsonPayloadError {
            e,
            bt: Trace::new(),
        }
    }
}
