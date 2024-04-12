#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

use ya_utils_process::SystemError;

use crate::error::CounterError;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::win::*;

pub(super) mod counters;

impl From<SystemError> for CounterError {
    fn from(error: SystemError) -> Self {
        CounterError::Other(error.to_string())
    }
}
