pub(crate) mod dao;
pub(crate) mod model;
pub(crate) mod schema;

#[allow(dead_code)]
pub(crate) mod migrations {
    #[derive(EmbedMigrations)]
    struct _Dummy;
}
