#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

pub mod models;
pub mod schema;

#[allow(dead_code)]
pub mod migrations {
    #[derive(EmbedMigrations)]
    struct _Dummy;
}
