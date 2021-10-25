use diesel::connection::SimpleConnection;
use diesel::migration::RunMigrationsError;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use diesel::{Connection, SqliteConnection};
use dotenv::dotenv;
use r2d2::CustomizeConnection;
use std::env;
use std::fmt::Display;
use std::path::Path;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct ProtectedPool {
    inner: Pool<ConnectionManager<InnerConnType>>,
    tx_lock: TxLock,
}

impl ProtectedPool {
    fn get(&self) -> Result<PooledConnection<ConnectionManager<InnerConnType>>, r2d2::Error> {
        self.inner.get()
    }
}

pub type PoolType = ProtectedPool;
type TxLock = Arc<RwLock<u64>>;
pub type ConnType = PooledConnection<ConnectionManager<InnerConnType>>;
pub type InnerConnType = SqliteConnection;

const CONNECTION_INIT: &str = r"
PRAGMA busy_timeout = 15000;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    DieselError(#[from] diesel::result::Error),
    #[error("{0}")]
    PoolError(#[from] r2d2::Error),
    #[error("task: {0}")]
    RuntimeError(#[from] tokio::task::JoinError),
    #[error("Serde Json error: {0}")]
    SerdeJsonError(#[from] serde_json::error::Error),
}

#[derive(Clone)]
pub struct DbExecutor {
    pub pool: PoolType,
}

fn connection_customizer(
    url: String,
    tx_lock: TxLock,
) -> impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> {
    #[derive(Debug)]
    struct ConnectionInit(TxLock, String);

    impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for ConnectionInit {
        fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
            let mut lock_cnt = self.0.write().unwrap();
            *lock_cnt += 1;
            log::trace!("on_acquire connection [rw:{}]", *lock_cnt);
            Ok(conn.batch_execute(CONNECTION_INIT).map_err(|e| {
                log::error!(
                    "error: {:?}, on: {}, [lock: {}]",
                    e,
                    self.1.as_str(),
                    *lock_cnt
                );
                diesel::r2d2::Error::QueryError(e)
            })?)
        }

        fn on_release(&self, _conn: SqliteConnection) {
            log::trace!("on_release connection");
        }
    }

    ConnectionInit(tx_lock, url)
}

// -

impl DbExecutor {
    pub fn new<S: Display>(database_url: S) -> Result<Self, Error> {
        DbExecutor::new_with_pool_size(database_url, None)
    }

    fn new_with_pool_size<S: Display>(
        database_url: S,
        pool_size: Option<u32>,
    ) -> Result<Self, Error> {
        let database_url = format!("{}", database_url);
        log::info!("using database at: {}", database_url);
        let manager = ConnectionManager::new(database_url.clone());
        let tx_lock: TxLock = Arc::new(RwLock::new(0));

        let builder = Pool::builder().connection_customizer(Box::new(connection_customizer(
            database_url.clone(),
            tx_lock.clone(),
        )));

        let inner = match pool_size {
            // Sqlite doesn't handle connections from multiple threads well.
            Some(pool_size) => builder.max_size(pool_size).build(manager)?,
            None => builder.build(manager)?,
        };

        {
            let connection = inner.get()?;
            let _ = connection.execute("PRAGMA journal_mode = WAL;")?;
        }

        let pool = ProtectedPool { inner, tx_lock };

        Ok(DbExecutor { pool })
    }

    pub fn from_env() -> Result<Self, Error> {
        dotenv().ok();

        let database_url = env::var_os("DATABASE_URL").unwrap_or("".into());
        Self::new(database_url.to_string_lossy())
    }

    pub fn from_data_dir(data_dir: &Path, name: &str) -> Result<Self, Error> {
        let db = data_dir.join(name).with_extension("db");
        Self::new(db.to_string_lossy())
    }

    pub fn in_memory(name: &str) -> Result<Self, Error> {
        Self::new_with_pool_size(format!("file:{}?mode=memory&cache=shared", name), Some(1))
    }

    fn conn(&self) -> Result<ConnType, Error> {
        Ok(self.pool.get()?)
    }

    pub fn as_dao<'a, T: AsDao<'a>>(&'a self) -> T {
        AsDao::as_dao(&self.pool)
    }

    pub fn apply_migration<
        T: FnOnce(&ConnType, &mut dyn std::io::Write) -> Result<(), RunMigrationsError>,
    >(
        &self,
        migration: T,
    ) -> anyhow::Result<()> {
        let c = self.conn()?;
        // Some migrations require disabling foreign key checks for advanced table manipulation.
        // Unfortunately, disabling foreign keys within migration doesn't work correctly.
        c.batch_execute("PRAGMA foreign_keys = OFF;")?;
        migration(&c, &mut std::io::stderr())?;
        c.batch_execute("PRAGMA foreign_keys = ON;")?;
        Ok(())
    }

    pub async fn with_connection<R: Send + 'static, Error, F>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&ConnType) -> Result<R, Error> + Send + 'static,
        Error: Send + 'static + From<tokio::task::JoinError> + From<r2d2::Error>,
    {
        do_with_ro_connection(&self.pool, f).await
    }

    pub async fn with_transaction<R: Send + 'static, Error, F>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&ConnType) -> Result<R, Error> + Send + 'static,
        Error: Send
            + 'static
            + From<tokio::task::JoinError>
            + From<r2d2::Error>
            + From<diesel::result::Error>,
    {
        do_with_transaction(&self.pool, f).await
    }

    #[allow(unused)]
    pub(crate) async fn execute(&self, query: &str) -> Result<usize, Error> {
        Ok(self.conn()?.execute(query)?)
    }
}

pub trait AsDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self;
}

async fn do_with_ro_connection<R: Send + 'static, Error, F>(
    pool: &PoolType,
    f: F,
) -> Result<R, Error>
where
    F: FnOnce(&ConnType) -> Result<R, Error> + Send + 'static,
    Error: Send + 'static + From<tokio::task::JoinError> + From<r2d2::Error>,
{
    let pool = pool.clone();
    match tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        let rw_cnt = pool.tx_lock.read().unwrap();
        //log::info!("start ro tx: {}", *rw_cnt);
        let ret = f(&conn);
        log::trace!("done ro tx: {}", *rw_cnt);
        ret
    })
    .await
    {
        Ok(v) => v,
        Err(join_err) => Err(From::from(join_err)),
    }
}

async fn do_with_rw_connection<R: Send + 'static, Error, F>(
    pool: &PoolType,
    f: F,
) -> Result<R, Error>
where
    F: FnOnce(&ConnType) -> Result<R, Error> + Send + 'static,
    Error: Send + 'static + From<tokio::task::JoinError> + From<r2d2::Error>,
{
    let pool = pool.clone();
    match tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        let _ = pool.tx_lock.read().unwrap();
        f(&conn)
    })
    .await
    {
        Ok(v) => v,
        Err(join_err) => Err(From::from(join_err)),
    }
}

pub async fn do_with_transaction<R: Send + 'static, Error, F>(
    pool: &PoolType,
    f: F,
) -> Result<R, Error>
where
    F: FnOnce(&ConnType) -> Result<R, Error> + Send + 'static,
    Error: Send
        + 'static
        + From<tokio::task::JoinError>
        + From<r2d2::Error>
        + From<diesel::result::Error>,
{
    do_with_rw_connection(pool, move |conn| conn.immediate_transaction(|| f(conn))).await
}

pub async fn readonly_transaction<R: Send + 'static, Error, F>(
    pool: &PoolType,
    f: F,
) -> Result<R, Error>
where
    F: FnOnce(&ConnType) -> Result<R, Error> + Send + 'static,
    Error: Send
        + 'static
        + From<tokio::task::JoinError>
        + From<r2d2::Error>
        + From<diesel::result::Error>,
{
    do_with_ro_connection(pool, move |conn| {
        conn.transaction(|| {
            #[cfg(debug_assertions)]
            let _ = conn.execute("PRAGMA query_only=1;")?;
            let result = f(conn);
            #[cfg(debug_assertions)]
            let _ = conn.execute("PRAGMA query_only=0;")?;
            result
        })
    })
    .await
}

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
        AsMixedDao::as_dao(&self.disk_db.pool, &self.ram_db.pool)
    }
}
