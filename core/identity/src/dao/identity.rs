pub use crate::db::models::Identity;
use crate::db::schema as s;
use diesel::prelude::*;
use tokio::task;
use ya_core_model::ethaddr::NodeId;
use ya_persistence::executor::{AsDao, ConnType, PoolType};

type Result<T> = std::result::Result<T, super::Error>;

pub struct IdentityDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for IdentityDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> IdentityDao<'c> {
    #[inline]
    async fn with_transaction<
        R: Send + 'static,
        F: FnOnce(&ConnType) -> Result<R> + Send + 'static,
    >(
        &self,
        f: F,
    ) -> Result<R> {
        self.with_connection(move |conn| conn.transaction(|| f(conn)))
            .await
    }

    #[inline]
    async fn with_connection<
        R: Send + 'static,
        F: FnOnce(&ConnType) -> Result<R> + Send + 'static,
    >(
        &self,
        f: F,
    ) -> Result<R> {
        let pool = self.pool.clone();
        match task::spawn_blocking(move || {
            let conn = pool.get()?;
            f(&conn)
        })
        .await
        {
            Ok(v) => v,
            Err(join_err) => Err(super::Error::internal(join_err)),
        }
    }

    pub async fn create_identity(&self, new_identity: Identity) -> Result<()> {
        let _rows = self
            .with_transaction(|conn| {
                Ok(diesel::insert_into(s::identity::table)
                    .values(new_identity)
                    .execute(conn)?)
            })
            .await?;
        Ok(())
    }

    pub async fn list_identities(&self) -> Result<Vec<Identity>> {
        use crate::db::schema::identity::dsl::*;
        self.with_connection(|conn| {
            Ok(identity
                .filter(is_default.eq(false))
                .load::<Identity>(conn)?)
        })
        .await
    }
}
