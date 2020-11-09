/*
    Base crate for creating payment drivers.

    Contains a trait, error stubs (TBD) and utils.
*/

#[macro_use]
extern crate diesel;

extern crate log;

pub mod account;
pub mod bus;
pub mod db;
pub mod dao;
pub mod driver;
pub mod utils;

pub use ya_core_model::driver as model;
