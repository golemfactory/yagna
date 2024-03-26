use std::{fmt::Display, sync::Arc};

use thiserror::Error;

#[derive(Clone, Debug, Error)]
pub enum MetricError {
    #[error("Metric unsupported: {0}")]
    Unsupported(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    // #[error(transparent)]
    // Other(#[from] ClonableError),
    #[error("Other error: {0}")]
    Other(String),
}

// #[derive(Clone, Debug, Error)]
// struct ClonableError(Arc<anyhow::Error>);

// impl Display for ClonableError {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{}", self.0)
//     }
// }

// impl From<anyhow::Error> for ClonableError {
//     fn from(err: anyhow::Error) -> Self {
//             ClonableError(Arc::new(err))
//     }
// }
