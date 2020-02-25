use actix_http::client::SendRequestError;
use actix_http::error::PayloadError;
use futures::channel::mpsc::SendError;

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
}

unsafe impl Send for Error {}

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

impl From<Error> for actix_http::error::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::HttpError(e) => match e {
                HttpError::PayloadError(e) => actix_http::error::Error::from(e),
                HttpError::SendRequestError(e) => actix_http::error::Error::from(e),
                HttpError::Unspecified => actix_http::error::Error::from(()),
            },
            Error::IoError(e) => actix_http::error::Error::from(e),
            Error::SendError(_) => actix_http::error::Error::from(()),
        }
    }
}
