use crate::error::Error;
use diesel::connection::SimpleConnection;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use diesel::SqliteConnection;

pub(crate) mod schema;

pub(crate) type ConnType = PooledConnection<ConnectionManager<InnerConnType>>;
pub(crate) type InnerConnType = SqliteConnection;

pub struct DbExecutor {
    pub pool: Pool<ConnectionManager<InnerConnType>>,
}

impl DbExecutor {
    pub fn new<S: Into<String>>(database_url: S) -> Result<Self, Error> {
        let manager = ConnectionManager::new(database_url);
        let pool = Pool::builder().build(manager)?;
        Ok(DbExecutor { pool })
    }

    pub fn conn(&self) -> Result<ConnType, Error> {
        let conn = self.pool.get()?;
        conn.batch_execute(
            "PRAGMA synchronous = NORMAL; PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;",
        )?;
        Ok(conn)
    }
}
