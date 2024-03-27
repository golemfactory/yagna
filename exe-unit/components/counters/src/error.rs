use std::{fmt::Display, rc::Rc, sync::Arc};

use thiserror::Error;

#[derive(Clone, Debug, Error)]
pub enum MetricError {
    #[error("Metric unsupported: {0}")]
    Unsupported(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    // #[error(transparent)]
    // Other(#[from] ClonedError),
    #[error("Other error: {0}")]
    Other(String),
    // #[error(transparent)]
    // Other(#[from] Arc<anyhow::Error>),
    // #[error(transparent)]
    // Other(#[from] Rc<anyhow::Error>),
}

// #[derive(Clone, Debug, Error)]
// struct ClonedError(Arc<anyhow::Error>);

// impl Display for ClonedError {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{}", self.0)
//     }
// }

// impl From<anyhow::Error> for ClonedError {
//     fn from(err: anyhow::Error) -> Self {
//             ClonedError(Arc::new(err))
//     }
// }

// impl From<anyhow::Error> for MetricError {
//     fn from(value: anyhow::Error) -> Self {
//         Self::Other(Rc::new(value))
//     }
// }
