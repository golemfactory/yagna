#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

mod notifier;
mod service;

pub use service::VersionService;
