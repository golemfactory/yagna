#[macro_use]
extern crate diesel;

pub mod executor;
pub mod types;

pub use executor::Error;
