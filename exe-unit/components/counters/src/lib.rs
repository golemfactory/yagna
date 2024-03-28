pub mod counters;
pub mod error;
pub mod message;
pub mod service;

pub type Result<T> = std::result::Result<T, error::MetricError>;
