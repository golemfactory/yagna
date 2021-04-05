/*
    Data access object for transaction, linking `TransactionEntity` with `transaction`
*/

// External crates
use diesel::{
    self, BoolExpressionMethods, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl,
};

// Workspace uses
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

// Local uses
use crate::{
    dao::DbResult,
    db::{
        models::{Network, TransactionEntity, TransactionStatus, TxType},
        schema::transaction::dsl,
    },
};

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
        readonly_transaction(self.pool, move |conn| {
            let tx: Option<TransactionEntity> =
                dsl::transaction.find(tx_id).first(conn).optional()?;
            Ok(tx)
        })
        .await
    }

    pub async fn insert_transactions(&self, txs: Vec<TransactionEntity>) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            for tx in txs {
                diesel::insert_into(dsl::transaction)
                    .values(tx)
                    .execute(conn)?;
            }
            Ok(())
        })
        .await
    }

    pub async fn get_used_nonces(&self, address: &str, network: Network) -> DbResult<Vec<String>> {
        let address = address.to_string();
        readonly_transaction(self.pool, move |conn| {
            let nonces: Vec<String> = dsl::transaction
                .filter(dsl::sender.eq(address).and(dsl::network.eq(network)))
                .select(dsl::nonce)
                .order(dsl::nonce.asc())
                .load(conn)?;
            Ok(nonces)
        })
        .await
    }

    pub async fn get_pending_faucet_txs(
        &self,
        node_id: &str,
        network: Network,
    ) -> DbResult<Vec<TransactionEntity>> {
        let node_id = node_id.to_string();
        readonly_transaction(self.pool, move |conn| {
            let txs: Vec<TransactionEntity> = dsl::transaction
                .filter(
                    dsl::tx_type
                        .eq(TxType::Faucet as i32)
                        .and(
                            dsl::status
                                .eq(TransactionStatus::Created as i32)
                                .or(dsl::status.eq(TransactionStatus::Sent as i32)),
                        )
                        .and(dsl::sender.eq(node_id))
                        .and(dsl::network.eq(network)),
                )
                .load(conn)?;
            Ok(txs)
        })
        .await
    }

    pub async fn get_unsent_txs(&self, network: Network) -> DbResult<Vec<TransactionEntity>> {
        self.get_by_status(TransactionStatus::Created, network)
            .await
    }

    pub async fn get_unconfirmed_txs(&self, network: Network) -> DbResult<Vec<TransactionEntity>> {
        self.get_by_status(TransactionStatus::Sent, network).await
    }

    pub async fn has_unconfirmed_txs(&self) -> DbResult<bool> {
        readonly_transaction(self.pool, move |conn| {
            let tx: Option<TransactionEntity> = dsl::transaction
                .filter(dsl::status.eq(TransactionStatus::Sent as i32))
                .first(conn)
                .optional()?;
            Ok(tx.is_some())
        })
        .await
    }

    pub async fn get_by_status(
        &self,
        status: TransactionStatus,
        network: Network,
    ) -> DbResult<Vec<TransactionEntity>> {
        readonly_transaction(self.pool, move |conn| {
            let txs: Vec<TransactionEntity> = dsl::transaction
                .filter(dsl::status.eq(status as i32).and(dsl::network.eq(network)))
                .load(conn)?;
            Ok(txs)
        })
        .await
    }

    pub async fn update_tx_sent(&self, tx_id: String, tx_hash: String) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::transaction.find(tx_id))
                .set((
                    dsl::status.eq(TransactionStatus::Sent as i32),
                    dsl::tx_hash.eq(tx_hash),
                ))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_tx_status(&self, tx_id: String, status: TransactionStatus) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::transaction.find(tx_id))
                .set(dsl::status.eq(status as i32))
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
