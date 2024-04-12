use thiserror::Error;
use ya_utils_process::SystemError;


#[derive(Clone, Debug, Error)]
pub enum CounterError {
    #[error("Counter unsupported: {0}")]
    Unsupported(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    #[error("Other error: {0}")]
    Other(String),
}

impl From<SystemError> for CounterError {
    fn from(error: SystemError) -> Self {
        CounterError::Other(error.to_string())
    }
}
