/*
    Raw database components. Schemas, models and migrations.
*/

pub mod migrations {
    pub const MIGRATIONS: diesel_migrations::EmbeddedMigrations =
        diesel_migrations::embed_migrations!();
}

pub mod models;
pub mod schema;
