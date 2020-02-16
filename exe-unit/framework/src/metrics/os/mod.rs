#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

#[cfg(unix)]
pub use self::unix::*;

#[cfg(windows)]
pub use self::win::*;
