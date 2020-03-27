#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

pub mod executor;
pub mod models;
pub mod schema;
pub mod types;

pub use executor::Error;

#[allow(dead_code)]
pub mod migrations {
    #[derive(EmbedMigrations)]
    struct _Dummy;
}
