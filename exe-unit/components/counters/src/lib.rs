pub mod error;
pub mod counters;
pub mod service;
pub mod message;

pub type Result<T> = std::result::Result<T, error::MetricError>;
