pub(crate) mod dao;
pub(crate) mod model;
pub(crate) mod schema;

pub(crate) mod migrations {
    #[derive(diesel_migrations::EmbedMigrations)]
    struct _Dummy;
}

pub(crate) use ya_persistence::executor::Error as DbError;
use ya_persistence::executor::{DbExecutor, PoolType};

pub(crate) type DbResult<T> = Result<T, DbError>;

#[derive(Clone)]
pub struct DbMixedExecutor {
    pub disk_db: DbExecutor,
    pub ram_db: DbExecutor,
}

pub trait AsMixedDao<'a> {
    fn as_dao(disk_pool: &'a PoolType, ram_pool: &'a PoolType) -> Self;
}

impl DbMixedExecutor {
    pub fn new(disk_db: DbExecutor, ram_db: DbExecutor) -> DbMixedExecutor {
        DbMixedExecutor { disk_db, ram_db }
    }

    pub fn as_dao<'a, T: AsMixedDao<'a>>(&'a self) -> T {
        AsMixedDao::as_dao(&self.ram_db.pool, &self.ram_db.pool)
    }
}
