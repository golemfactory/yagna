/*
    Base crate for creating payment drivers.

    Contains a trait, error stubs (TBD) and utils.
*/

#![allow(non_local_definitions)] // Due to Diesel macros.

#[macro_use]
extern crate diesel;

#[macro_use]
extern crate num_derive;

extern crate log;

pub mod bus;
pub mod cron;
pub mod dao;
pub mod db;
pub mod driver;
pub mod utils;

pub use ya_core_model::driver as model;
