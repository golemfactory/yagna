pub use crate::db::models::Identity;
use crate::db::schema as s;
use ya_persistence::executor::{PoolType, AsDao};
use diesel::r2d2::ConnectionManager;
use diesel::{SqliteConnection, Connection, RunQueryDsl, QueryDsl};
use ya_core_model::ethaddr::NodeId;
use tokio::task;
use std::convert::identity;

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

    pub async fn create_identity(&self, new_identity : Identity) -> Result<()> {
        let pool = self.pool.clone();
        let _ = task::spawn_blocking(move || {
            let conn = pool.get()?;

            conn.transaction(|| {
                eprintln!("in transaction");
                Ok(diesel::insert_into(s::identity::table)
                    .values(new_identity)
                    .execute(&conn)?)
            }) as Result<_>
        }).await.unwrap();
        eprintln!("done");
        Ok(())
    }

    pub async fn list_identitys(&self) -> Result<Vec<Identity>> {
        let pool = self.pool.clone();
        task::spawn_blocking(move || {
            use crate::db::schema::identity::dsl::*;
            use diesel::prelude::*;
            let conn = pool.get()?;
            let results = identity.filter(is_default.eq(false)).load::<Identity>(&conn)?;
            Ok(results)
        }).await.unwrap()
    }

}