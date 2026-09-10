use diesel::connection::SimpleConnection;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use diesel::{Connection, SqliteConnection};
use diesel_migrations::{EmbeddedMigrations, HarnessWithOutput, MigrationHarness};
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
pub type InnerConnType = SqliteConnection;
pub type ConnType = InnerConnType;
type PooledConnType = PooledConnection<ConnectionManager<InnerConnType>>;

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
            conn.batch_execute(CONNECTION_INIT).map_err(|e| {
                log::error!(
                    "error: {:?}, on: {}, [lock: {}]",
                    e,
                    self.1.as_str(),
                    *lock_cnt
                );
                diesel::r2d2::Error::QueryError(e)
            })
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
            database_url,
            tx_lock.clone(),
        )));

        let inner = match pool_size {
            // Sqlite doesn't handle connections from multiple threads well.
            Some(pool_size) => builder
                .max_size(pool_size)
                .idle_timeout(None)
                .max_lifetime(None)
                .build(manager)?,
            None => builder.build(manager)?,
        };

        {
            let mut connection = inner.get()?;
            connection.batch_execute("PRAGMA journal_mode = WAL;")?;
        }

        let pool = ProtectedPool { inner, tx_lock };

        Ok(DbExecutor { pool })
    }

    pub fn from_env() -> Result<Self, Error> {
        dotenv().ok();

        let database_url = env::var_os("DATABASE_URL").unwrap_or_else(|| "".into());
        Self::new(database_url.to_string_lossy())
    }

    pub fn from_data_dir(data_dir: &Path, name: &str) -> Result<Self, Error> {
        let db = data_dir.join(name).with_extension("db");
        Self::new(db.to_string_lossy())
    }

    pub fn in_memory(name: &str) -> Result<Self, Error> {
        Self::new_with_pool_size(format!("file:{}?mode=memory&cache=shared", name), Some(1))
    }

    fn conn(&self) -> Result<PooledConnType, Error> {
        Ok(self.pool.get()?)
    }

    pub fn as_dao<'a, T: AsDao<'a>>(&'a self) -> T {
        AsDao::as_dao(&self.pool)
    }

    pub fn apply_migration(&self, migrations: EmbeddedMigrations) -> anyhow::Result<()> {
        let mut c = self.conn()?;
        // Some migrations require disabling foreign key checks for advanced table manipulation.
        // Unfortunately, disabling foreign keys within migration doesn't work correctly.
        c.batch_execute("PRAGMA foreign_keys = OFF;")?;
        let migration_result = {
            let mut harness = HarnessWithOutput::new(&mut *c, std::io::stderr());
            harness
                .run_pending_migrations(migrations)
                .map(|_| ())
                .map_err(|error| anyhow::anyhow!("failed to apply migrations: {error}"))
        };
        let restore_result = c
            .batch_execute("PRAGMA foreign_keys = ON;")
            .map_err(|error| anyhow::anyhow!("failed to re-enable foreign keys: {error}"));

        match (migration_result, restore_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(migration_error), Ok(())) => Err(migration_error),
            (Ok(()), Err(restore_error)) => Err(restore_error),
            (Err(migration_error), Err(restore_error)) => Err(anyhow::anyhow!(
                "{migration_error}; additionally, {restore_error}"
            )),
        }
    }

    // not used in yagna, but may be useful in other projects
    async fn _with_connection<R: Send + 'static, Error, F>(
        &self,
        label: &'static str,
        f: F,
    ) -> Result<R, Error>
    where
        F: FnOnce(&mut ConnType) -> Result<R, Error> + Send + 'static,
        Error: Send + 'static + From<tokio::task::JoinError> + From<r2d2::Error>,
    {
        do_with_ro_connection(&self.pool, label, f).await
    }

    pub async fn with_transaction<R: Send + 'static, Error, F>(
        &self,
        label: &'static str,
        f: F,
    ) -> Result<R, Error>
    where
        F: FnOnce(&mut ConnType) -> Result<R, Error> + Send + 'static,
        Error: Send
            + 'static
            + From<tokio::task::JoinError>
            + From<r2d2::Error>
            + From<diesel::result::Error>,
    {
        do_with_transaction(&self.pool, label, f).await
    }

    #[allow(unused)]
    pub(crate) async fn execute(&self, query: &str) -> Result<(), Error> {
        // Keep Diesel 1.x `Connection::execute` semantics: execute every
        // statement in the string, not only the first one.
        Ok(self.conn()?.batch_execute(query)?)
    }
}

pub trait AsDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self;
}
static RO_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

async fn do_with_ro_connection<R: Send + 'static, Error, F>(
    pool: &PoolType,
    label: &'static str,
    f: F,
) -> Result<R, Error>
where
    F: FnOnce(&mut ConnType) -> Result<R, Error> + Send + 'static,
    Error: Send + 'static + From<tokio::task::JoinError> + From<r2d2::Error>,
{
    let pool = pool.clone();

    let count_no = RO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    match tokio::task::spawn_blocking(move || {
        let mut conn = pool.get()?;
        log::trace!("Start ro transaction no {}: {}", count_no, label);

        let rw_cnt = pool.tx_lock.read().unwrap();
        //log::info!("start ro tx: {}", *rw_cnt);
        let start_query = std::time::Instant::now();
        let ret = f(&mut conn);
        let end_query = std::time::Instant::now();
        //log::trace!("done ro tx: {}", *rw_cnt);
        drop(rw_cnt);
        if ret.is_err() {
            log::trace!(
                "Error in ro transaction no: {}: {}, time: {}ms",
                count_no,
                label,
                end_query.duration_since(start_query).as_millis()
            );
        } else {
            log::trace!(
                "Finished ro transaction no: {}: {}, time: {}ms",
                count_no,
                label,
                end_query.duration_since(start_query).as_millis()
            );
        }
        ret
    })
    .await
    {
        Ok(v) => v,
        Err(join_err) => Err(From::from(join_err)),
    }
}

static RW_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

async fn do_with_rw_connection<R: Send + 'static, Error, F>(
    pool: &PoolType,
    label: &'static str,
    f: F,
) -> Result<R, Error>
where
    F: FnOnce(&mut ConnType) -> Result<R, Error> + Send + 'static,
    Error: Send + 'static + From<tokio::task::JoinError> + From<r2d2::Error>,
{
    //log::warn!("DB read write transaction: {}", label);

    let count_no = RW_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    //log::warn!("Do_with_rw_connection {count_no}");
    let pool = pool.clone();
    match tokio::task::spawn_blocking(move || {
        let mut conn = pool.get()?;
        log::trace!("Start rw transaction no {}: {}", count_no, label);
        let _guard = pool.tx_lock.write().unwrap();
        let start_query = std::time::Instant::now();
        let res = f(&mut conn);
        let end_query = std::time::Instant::now();
        drop(_guard);
        if res.is_err() {
            log::trace!(
                "Error in rw transaction no: {}: {}, time: {}ms",
                count_no,
                label,
                end_query.duration_since(start_query).as_millis()
            );
        } else {
            log::trace!(
                "Finished rw transaction no: {}: {}, time: {}ms",
                count_no,
                label,
                end_query.duration_since(start_query).as_millis()
            );
        }
        res
    })
    .await
    {
        Ok(v) => v,
        Err(join_err) => Err(From::from(join_err)),
    }
}

pub async fn do_with_transaction<R: Send + 'static, Error, F>(
    pool: &PoolType,
    label: &'static str,
    f: F,
) -> Result<R, Error>
where
    F: FnOnce(&mut ConnType) -> Result<R, Error> + Send + 'static,
    Error: Send
        + 'static
        + From<tokio::task::JoinError>
        + From<r2d2::Error>
        + From<diesel::result::Error>,
{
    do_with_rw_connection(pool, label, move |conn| conn.immediate_transaction(f)).await
}

#[allow(clippy::let_and_return)]
pub async fn readonly_transaction<R: Send + 'static, Error, F>(
    pool: &PoolType,
    label: &'static str,
    f: F,
) -> Result<R, Error>
where
    F: FnOnce(&mut ConnType) -> Result<R, Error> + Send + 'static,
    Error: Send
        + 'static
        + From<tokio::task::JoinError>
        + From<r2d2::Error>
        + From<diesel::result::Error>,
{
    do_with_ro_connection(pool, label, move |conn| {
        conn.transaction(|conn| {
            #[cfg(debug_assertions)]
            conn.batch_execute("PRAGMA query_only=1;")?;
            let result = f(conn);
            #[cfg(debug_assertions)]
            conn.batch_execute("PRAGMA query_only=0;")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::sql_types::{BigInt, Integer};
    use diesel::{QueryableByName, RunQueryDsl};

    const FAILING_MIGRATIONS: EmbeddedMigrations =
        diesel_migrations::embed_migrations!("tests/migrations/failing");

    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = BigInt)]
        count: i64,
    }

    #[derive(QueryableByName)]
    struct ForeignKeys {
        #[diesel(sql_type = Integer)]
        foreign_keys: i32,
    }

    #[tokio::test]
    async fn execute_runs_every_statement() {
        let db = DbExecutor::in_memory("executor-runs-every-statement").unwrap();

        db.execute(
            "CREATE TABLE items (id INTEGER PRIMARY KEY); \
             INSERT INTO items (id) VALUES (1); \
             INSERT INTO items (id) VALUES (2);",
        )
        .await
        .unwrap();

        let count = diesel::sql_query("SELECT COUNT(*) AS count FROM items")
            .get_result::<Count>(&mut *db.conn().unwrap())
            .unwrap();
        assert_eq!(count.count, 2);
    }

    #[test]
    fn failed_migration_reenables_foreign_keys() {
        let db = DbExecutor::in_memory("failed-migration-reenables-foreign-keys").unwrap();

        let error = db.apply_migration(FAILING_MIGRATIONS).unwrap_err();
        assert!(error.to_string().contains("failed to apply migrations"));

        let setting = diesel::sql_query("PRAGMA foreign_keys")
            .get_result::<ForeignKeys>(&mut *db.conn().unwrap())
            .unwrap();
        assert_eq!(setting.foreign_keys, 1);
    }
}
