use diesel::connection::SimpleConnection;
use diesel::migration::RunMigrationsError;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use diesel::{Connection, SqliteConnection};
use dotenv::dotenv;
use r2d2::CustomizeConnection;
use std::env;
use std::path::Path;
use std::sync::Mutex;

pub type PoolType = Pool<ConnectionManager<InnerConnType>>;
pub type ConnType = PooledConnection<ConnectionManager<InnerConnType>>;
pub type InnerConnType = SqliteConnection;

const CONNECTION_INIT: &str = r"
PRAGMA synchronous = NORMAL;
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 15000;
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
    pub pool: Pool<ConnectionManager<InnerConnType>>,
}

fn connection_customizer() -> impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> {
    #[derive(Debug)]
    struct ConnectionInit(Mutex<()>);

    impl CustomizeConnection<SqliteConnection, diesel::r2d2::Error> for ConnectionInit {
        fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
            let _lock = self.0.lock().unwrap();
            log::trace!("on_acquire connection");
            Ok(conn
                .batch_execute(CONNECTION_INIT)
                .map_err(diesel::r2d2::Error::QueryError)?)
        }

        fn on_release(&self, _conn: SqliteConnection) {
            log::trace!("on_release connection");
        }
    }

    ConnectionInit(Mutex::new(()))
}

impl DbExecutor {
    pub fn new<S: Into<String>>(database_url: S) -> Result<Self, Error> {
        let database_url = database_url.into();
        log::info!("using database at: {}", database_url);
        let manager = ConnectionManager::new(database_url);
        let pool = Pool::builder()
            .connection_customizer(Box::new(connection_customizer()))
            .build(manager)?;
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

    pub fn conn(&self) -> Result<ConnType, Error> {
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
        do_with_connection(&self.pool, f).await
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
        self.with_connection(|conn| conn.transaction(move || f(conn)))
            .await
    }
}

pub trait AsDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self;
}

pub async fn do_with_connection<R: Send + 'static, Error, F>(
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
    do_with_connection(pool, move |conn| conn.immediate_transaction(|| f(conn))).await
}

#[cfg(debug_assertions)]
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
    do_with_connection(pool, move |conn| {
        conn.transaction(|| {
            let _ = conn.execute("PRAGMA query_only=1;")?;
            let result = f(conn);
            let _ = conn.execute("PRAGMA query_only=0;")?;
            result
        })
    })
    .await
}

#[cfg(not(debug_assertions))]
#[inline]
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
    do_with_connection(pool, f).await
}
