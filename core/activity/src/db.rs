pub(crate) mod models;
pub(crate) mod schema;

#[allow(dead_code)]
pub mod migrations {
    #[derive(EmbedMigrations)]
    struct _Dummy;
}
