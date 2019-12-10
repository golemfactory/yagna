pub mod activity;
pub mod market;

mod error;
pub use error::Error;

pub type Result<T> = std::result::Result<T, Error>;
