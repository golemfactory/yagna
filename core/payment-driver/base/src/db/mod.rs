/*
    Raw database components. Schemas, models and migrations.
*/

pub mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

pub mod models;
pub mod schema;
