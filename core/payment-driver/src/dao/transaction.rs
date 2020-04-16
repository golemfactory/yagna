use diesel::{self, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use crate::error::DbResult;
use crate::models::{TransactionEntity, TransactionStatus};
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
    pub async fn get(&self, tx_id: String) -> DbResult<Option<TransactionEntity>> {
        do_with_transaction(self.pool, move |conn| {
            let tx: Option<TransactionEntity> = dsl::gnt_driver_transaction
                .find(tx_id.clone())
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

    pub async fn get_used_nonces(&self, address: String) -> DbResult<Vec<String>> {
        do_with_transaction(self.pool, move |conn| {
            let nonces: Vec<String> = dsl::gnt_driver_transaction
                .filter(dsl::sender.eq(address.clone()))
                .select(dsl::nonce)
                .load(conn)?;
            Ok(nonces)
        })
        .await
    }

    pub async fn update_tx_sent(&self, tx_id: String, tx_hash: String) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let sent_status: i32 = TransactionStatus::Sent.into();
            diesel::update(dsl::gnt_driver_transaction.find(tx_id.clone()))
                .set((dsl::status.eq(sent_status), dsl::tx_hash.eq(tx_hash)))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_tx_status(&self, tx_id: String, status: i32) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::gnt_driver_transaction.find(tx_id.clone()))
                .set(dsl::status.eq(status))
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
