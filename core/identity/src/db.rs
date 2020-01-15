pub(crate) mod models;
pub(crate) mod schema;

#[allow(dead_code)]
pub mod migrations {
    use diesel_migrations::*;
    #[derive(EmbedMigrations)]
    struct _Dummy;
}
