use actix_http::error::{ParseError, PayloadError};
use actix_web::ResponseError;
use awc::error::{ConnectError, SendRequestError};
use futures::channel::mpsc::SendError;
use futures::channel::oneshot::Canceled;
use futures::future::Aborted;
use std::io;
use std::io::ErrorKind;

#[derive(thiserror::Error, Debug)]
pub enum HttpError {
    #[error("io error: {0:?}")]
    Io(io::Error),
    #[error("connection error: {0}")]
    Connect(String),
    #[error("client error: {0}")]
    Client(String),
    #[error("server error: {0}")]
    Server(String),
    #[error("payload error: {0}")]
    Payload(PayloadError),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("{0}")]
    Other(String),
}

impl From<PayloadError> for HttpError {
    fn from(error: PayloadError) -> Self {
        match error {
            PayloadError::Io(io_err) => HttpError::Io(io_err),
            payload_err => HttpError::Payload(payload_err),
        }
    }
}

impl From<SendRequestError> for HttpError {
    fn from(error: SendRequestError) -> Self {
        match error {
            SendRequestError::Timeout => HttpError::Timeout("operation timed out".into()),
            SendRequestError::Send(e) => HttpError::Io(e),
            SendRequestError::Connect(e) => match e {
                ConnectError::Io(e) => HttpError::Io(e),
                ConnectError::Timeout => HttpError::Timeout("connection".into()),
                e => HttpError::Connect(e.to_string()),
            },
            SendRequestError::Response(e) => match e {
                ParseError::Io(e) => HttpError::Io(e),
                ParseError::Timeout => HttpError::Timeout("response read".into()),
                e => HttpError::Server(e.to_string()),
            },
            SendRequestError::Body(e) => {
                if e.status_code().is_server_error() {
                    HttpError::Server(e.to_string())
                } else {
                    HttpError::Client(e.to_string())
                }
            }
            SendRequestError::H2(e) => {
                use h2::Reason;

                if let Some(e) = e.get_io() {
                    return HttpError::Other(format!("IO error: {e}"));
                }
                if let Some(r) = e.reason() {
                    return match r {
                        Reason::CANCEL => {
                            HttpError::Io(io::Error::from(ErrorKind::ConnectionAborted))
                        }
                        Reason::STREAM_CLOSED => {
                            HttpError::Io(io::Error::from(ErrorKind::ConnectionAborted))
                        }
                        Reason::REFUSED_STREAM => {
                            HttpError::Io(io::Error::from(ErrorKind::ConnectionRefused))
                        }
                        Reason::CONNECT_ERROR => {
                            HttpError::Io(io::Error::from(ErrorKind::ConnectionReset))
                        }
                        Reason::SETTINGS_TIMEOUT => HttpError::Timeout("http/2 settings".into()),
                        Reason::NO_ERROR | Reason::INTERNAL_ERROR => {
                            HttpError::Server(format!("http/2 code: {}", r))
                        }
                        r => HttpError::Client(format!("http/2 code: {}", r)),
                    };
                }

                HttpError::Other(e.to_string())
            }
            e => HttpError::Other(e.to_string()),
        }
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
    #[error("GSB error: {0}")]
    Gsb(#[from] ya_service_bus::error::Error),
    #[error("gftp error: {0}")]
    Gftp(#[from] ya_core_model::gftp::Error),
    #[error("Glob error: {0}")]
    PathGlob(#[from] globset::Error),
    #[error("Invalid output format: {0}")]
    OutputFormat(String),
    #[error("URL parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("Invalid url: {0}")]
    InvalidUrlError(String),
    #[error("Unsupported scheme: {0}")]
    UnsupportedSchemeError(String),
    #[error("Unsupported digest: {0}")]
    UnsupportedDigestError(String),
    #[error("Incorrect hash provided or downloaded image is corrupted: calculated hash {hash} differs from the expected one {expected}")]
    InvalidHashError { hash: String, expected: String },
    #[error("Hex error: {0}")]
    HexError(#[from] hex::FromHexError),
    #[error("Net API error: {0}")]
    NetApiError(#[from] ya_core_model::net::NetApiError),
    #[error("Cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

impl ResponseError for Error {}

impl From<Aborted> for Error {
    fn from(_: Aborted) -> Self {
        Error::IoError(std::io::Error::from(std::io::ErrorKind::Interrupted))
    }
}

impl From<Canceled> for Error {
    fn from(_: Canceled) -> Self {
        Error::Cancelled
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

impl From<Error> for std::io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::IoError(error) => error,
            _ => std::io::Error::new(std::io::ErrorKind::Other, e),
        }
    }
}
