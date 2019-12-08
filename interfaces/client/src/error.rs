#[derive(failure::Fail, Debug)]
pub enum Error {
    #[fail(display = "AWC sending request error: {}", _0)]
    SendRequestError(String),
    #[fail(display = "AWC payload error: {}", _0)]
    PayloadError(awc::error::PayloadError),
    #[fail(display = "AWC JSON payload error: {}", _0)]
    JsonPayloadError(awc::error::JsonPayloadError),
    #[fail(display = "serde JSON error: {}", _0)]
    SerdeJsonError(serde_json::Error),
    #[fail(display = "invalid address: {}", _0)]
    InvalidAddress(#[fail(cause)] std::convert::Infallible),
    #[fail(display = "invalid UTF8 string: {}", _0)]
    InvalidString(#[fail(cause)] std::string::FromUtf8Error),
}

impl From<awc::error::SendRequestError> for Error {
    fn from(e: awc::error::SendRequestError) -> Self {
        Error::SendRequestError(format!("{:?}", e))
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
