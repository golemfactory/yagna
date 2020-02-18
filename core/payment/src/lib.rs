#![allow(dead_code)] // Crate under development
#![allow(unused_variables)] // Crate under development

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
#[macro_use]
extern crate ya_service_bus;

pub mod api;
pub mod dao;
pub mod error;
pub mod models;
pub mod schema;
pub mod service;
pub mod utils;

pub mod migrations {
    #[derive(EmbedMigrations)]
    struct _Dummy;
}
