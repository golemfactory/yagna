#[derive(failure::Fail, Debug)]
pub enum Error {
    #[fail(display = "error sending request: {}", _1)]
    SendRequestError(awc::error::SendRequestError, String),
    PayloadError(awc::error::PayloadError),
    JsonPayloadError(awc::error::JsonPayloadError),
    SerdeJsonError(serde_json::Error),
    #[fail(display = "invalid address: {}", _0)]
    InvalidAddress(#[fail(cause)] url::ParseError),
}

impl From<awc::error::SendRequestError> for Error {
    fn from(e: awc::error::SendRequestError) -> Self {
        Error::SendRequestError(e, String::new())
    }
}

impl From<awc::error::PayloadError> for Error {
    fn from(e: awc::error::PayloadError) -> Self {
        Error::PayloadError(e)
    }
}

impl From<awc::error::JsonPayloadError> for Error {
    fn from(e: awc::error::JsonPayloadError) -> Self {
        Error::JsonPayloadError(e)
    }
}
