pub(crate) mod dao;
pub(crate) mod model;
pub(crate) mod schema;

pub(crate) mod migrations {
    pub const MIGRATIONS: diesel_migrations::EmbeddedMigrations =
        diesel_migrations::embed_migrations!();
}

pub(crate) use ya_persistence::executor::Error as DbError;
pub(crate) use ya_persistence::executor::{AsMixedDao, DbMixedExecutor};

pub(crate) type DbResult<T> = Result<T, DbError>;
