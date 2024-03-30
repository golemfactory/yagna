pub mod error;
pub mod message;
#[cfg(feature = "os")]
pub mod os;
pub mod service;

mod counters;

pub use crate::counters::*;
#[cfg(feature = "os")]
pub use crate::os::counters::*;

pub type Result<T> = std::result::Result<T, error::MetricError>;
