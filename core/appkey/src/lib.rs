#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

pub mod cli;
pub(crate) mod dao;
pub(crate) mod db;
pub mod error;
pub mod service;

#[allow(dead_code)]
pub mod migrations {
    #[derive(EmbedMigrations)]
    struct _Dummy;
}
