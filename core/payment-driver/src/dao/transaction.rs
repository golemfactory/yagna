use diesel::{self, OptionalExtension, QueryDsl, RunQueryDsl};

use crate::error::DbResult;
use crate::models::TransactionEntity;
use crate::schema::gnt_driver_transaction::dsl;

use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

#[allow(unused)]
pub struct TransactionDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for TransactionDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> TransactionDao<'c> {
    pub async fn get(&self, tx_hash: String) -> DbResult<Option<TransactionEntity>> {
        do_with_transaction(self.pool, move |conn| {
            let tx: Option<TransactionEntity> = dsl::gnt_driver_transaction
                .find(tx_hash.clone())
                .first(conn)
                .optional()?;
            match tx {
                Some(tx) => Ok(Some(tx)),
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn insert(&self, tx: TransactionEntity) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::gnt_driver_transaction)
                .values(tx)
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
