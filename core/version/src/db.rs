pub(crate) mod dao;
pub(crate) mod model;
pub(crate) mod schema;

#[allow(dead_code)]
pub(crate) mod migrations {
    pub const MIGRATIONS: diesel_migrations::EmbeddedMigrations =
        diesel_migrations::embed_migrations!();
}
