use crate::metrics::os::SystemError;
use thiserror::Error;

#[derive(Clone, Debug, Error)]
pub enum MetricError {
    #[error("Metric error: {0}")]
    SystemError(#[from] SystemError),
    #[error("Metric unsupported")]
    Unsupported,
}
