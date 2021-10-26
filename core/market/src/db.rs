pub(crate) mod dao;
pub(crate) mod model;
pub(crate) mod schema;

pub(crate) mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

pub(crate) use ya_persistence::executor::Error as DbError;
pub(crate) use ya_persistence::executor::{AsMixedDao, DbMixedExecutor};

pub(crate) type DbResult<T> = Result<T, DbError>;
