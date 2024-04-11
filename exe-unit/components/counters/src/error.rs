use thiserror::Error;

#[cfg(feature = "os")]
use ya_process::SystemError;

#[derive(Clone, Debug, Error)]
pub enum CounterError {
    #[error("Counter unsupported: {0}")]
    Unsupported(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    #[error("Other error: {0}")]
    Other(String),
}

#[cfg(feature = "os")]
impl From<SystemError> for CounterError {
    fn from(error: SystemError) -> Self {
        match error {
            SystemError::NullPointer(err) => CounterError::Other(err.to_string()),
            SystemError::PoisonError => CounterError::Other("PoisonError".into()),
            SystemError::ApiError(err) => CounterError::Other(err.to_string()),
        }
    }
}
