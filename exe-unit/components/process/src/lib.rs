#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

#[cfg(unix)]
pub use self::unix::*;
#[cfg(windows)]
pub use self::win::*;

#[derive(Clone, Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("Unsupported: {0}")]
    Unsupported(String),
    #[error("Usage limit exceeded: {0}")]
    UsageLimitExceeded(String),
    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ProcessError>;
