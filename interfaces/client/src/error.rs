//! Error definitions and mappings
use awc::error::{JsonPayloadError, PayloadError, SendRequestError};
use awc::http::StatusCode;
use backtrace::Backtrace as Trace; // needed b/c of thiserror magic
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("AWC error requesting {url}: {e}")]
    SendRequestError { e: String, url: String, bt: Trace },
    #[error("AWC timeout requesting {url}: {e}")]
    TimeoutError { e: String, url: String, bt: Trace },
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

impl From<SendRequestError> for Error {
    fn from(e: SendRequestError) -> Self {
        match e {
            SendRequestError::Timeout => Error::TimeoutError {
                e: format!("{}", e),
                url: "".into(),
                bt: Trace::new(),
            },
            e => Error::SendRequestError {
                e: format!("{}", e),
                url: "".into(),
                bt: Trace::new(),
            },
        }
    }
}

impl From<(SendRequestError, String)> for Error {
    fn from((e, url): (SendRequestError, String)) -> Self {
        match e {
            SendRequestError::Timeout => Error::TimeoutError {
                e: format!("{}", e),
                url,
                bt: Trace::new(),
            },
            e => Error::SendRequestError {
                e: format!("{}", e),
                url,
                bt: Trace::new(),
            },
        }
    }
}

impl From<PayloadError> for Error {
    fn from(e: PayloadError) -> Self {
        Error::PayloadError {
            e,
            bt: Trace::new(),
        }
    }
}

impl From<JsonPayloadError> for Error {
    fn from(e: JsonPayloadError) -> Self {
        Error::JsonPayloadError {
            e,
            bt: Trace::new(),
        }
    }
}

impl From<(StatusCode, String)> for Error {
    fn from((status_code, url): (StatusCode, String)) -> Self {
        if status_code == StatusCode::REQUEST_TIMEOUT {
            Error::TimeoutError {
                e: format!("{:?}", status_code),
                url,
                bt: Trace::new(),
            }
        } else {
            Error::HttpStatusCode(status_code)
        }
    }
}
