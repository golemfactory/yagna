/*
    Base crate for creating payment drivers.

    Contains a trait, error stubs (TBD) and utils.
*/

extern crate log;

pub mod account;
pub mod bus;
pub mod driver;
pub mod utils;

pub use ya_core_model::driver as model;
