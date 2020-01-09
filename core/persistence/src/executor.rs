use diesel::connection::SimpleConnection;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use diesel::SqliteConnection;
use dotenv::dotenv;
use std::env;
use std::marker::PhantomData;

pub type ConnType = PooledConnection<ConnectionManager<InnerConnType>>;
pub type InnerConnType = SqliteConnection;

pub struct DbExecutor<E>
where
    E: From<diesel::result::Error> + From<r2d2::Error>,
{
    pub pool: Pool<ConnectionManager<InnerConnType>>,
    phantom_data: PhantomData<E>,
}

impl<E> DbExecutor<E>
where
    E: From<diesel::result::Error> + From<r2d2::Error>,
{
    pub fn new<S: Into<String>>(database_url: S) -> Result<Self, E> {
        let manager = ConnectionManager::new(database_url);
        let pool = Pool::builder().build(manager)?;
        Ok(DbExecutor {
            pool,
            phantom_data: PhantomData,
        })
    }

    pub fn from_env() -> Result<Self, E> {
        dotenv().ok();

        let database_url = env::var_os("DATABASE_URL").unwrap_or("".into());
        Self::new(database_url.to_string_lossy())
    }

    pub fn conn(&self) -> Result<ConnType, E> {
        let conn = self.pool.get()?;
        conn.batch_execute(
            "PRAGMA synchronous = NORMAL; PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;",
        )?;
        Ok(conn)
    }
}
