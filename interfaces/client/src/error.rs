//! Error definitions and mappings
use awc::error::{JsonPayloadError, PayloadError, SendRequestError};
use awc::http::StatusCode;
use backtrace::Backtrace as Trace; // needed b/c of thiserror magic
use thiserror::Error;
use ya_model::ErrorMessage;

#[derive(Error, Debug)]
pub enum Error {
    #[error("AWC error requesting {url}: {e}")]
    SendRequestError { e: String, url: String, bt: Trace },
    #[error("AWC timeout requesting {url}: {e}")]
    TimeoutError { e: String, url: String, bt: Trace },
    #[error("AWC payload error: {e}")]
    PayloadError { e: PayloadError, bt: Trace },
    #[error("AWC JSON payload error: {e}")]
    JsonPayloadError { e: JsonPayloadError, bt: Trace },
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::error::Error),
    #[error("request for {url} resulted in HTTP status code: {code}: {msg}")]
    HttpStatusCode {
        code: StatusCode,
        url: String,
        msg: String,
    },
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

impl From<(StatusCode, String, ErrorMessage)> for Error {
    fn from((code, url, err_msg): (StatusCode, String, ErrorMessage)) -> Self {
        if code == StatusCode::REQUEST_TIMEOUT {
            Error::TimeoutError {
                e: format!("{:?}", code),
                url,
                bt: Trace::new(),
            }
        } else {
            Error::HttpStatusCode {
                code,
                url,
                msg: err_msg.message.unwrap_or_default(),
            }
        }
    }
}
