use thiserror::Error;

#[derive(Clone, Debug, Error)]
pub enum MetricError {
    #[error("Metric unsupported: {0}")]
    Unsupported(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    #[error("Other error: {0}")]
    Other(String),
}
