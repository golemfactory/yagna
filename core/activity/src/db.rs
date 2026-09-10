pub(crate) mod models;
pub(crate) mod schema;

#[allow(dead_code)]
pub mod migrations {
    pub const MIGRATIONS: diesel_migrations::EmbeddedMigrations =
        diesel_migrations::embed_migrations!();
}
