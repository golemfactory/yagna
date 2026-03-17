#![allow(non_local_definitions)] // Due to Diesel macros.

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

mod db;
mod github;
mod notifier;
mod service;

pub use service::VersionService;
