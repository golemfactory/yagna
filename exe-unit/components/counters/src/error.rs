// use crate::process::SystemError;
use std::time::SystemTimeError;
use thiserror::Error;

#[derive(Clone, Debug, Error)]
pub enum MetricError {
    // #[error("Metric error: {0}")]
    // SystemError(#[from] SystemError),
    #[error("System time error: {0}")]
    SystemTimeError(#[from] SystemTimeError),
    #[error("Metric unsupported: {0}")]
    Unsupported(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
}
