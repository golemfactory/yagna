#![allow(dead_code)] // Crate under development
#![allow(unused_variables)] // Crate under development

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

pub mod api;
pub mod models;
pub mod schema;
pub mod service;

#[allow(dead_code)]
pub mod migrations {
    #[derive(EmbedMigrations)]
    struct _Dummy;
}
