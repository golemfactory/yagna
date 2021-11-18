/*
    Data access object for transaction, linking `TransactionEntity` with `transaction`
*/

// External crates
use chrono::Duration;
use chrono::NaiveDateTime;
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
use chrono::Utc;

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

    pub async fn get_used_nonces(&self, address: &str, network: Network) -> DbResult<Vec<i32>> {
        let address = address.to_string();
        let not_older_than = (Utc::now() - Duration::days(7)).naive_utc();
        readonly_transaction(self.pool, move |conn| {
            let nonces: Vec<i32> = dsl::transaction
                .filter(
                    dsl::sender
                        .eq(address)
                        .and(dsl::network.eq(network))
                        .and(dsl::time_created.gt(not_older_than)),
                )
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
        self.get_by_statuses(
            TransactionStatus::Created,
            TransactionStatus::Resend,
            TransactionStatus::ResendAndBumpGas,
            network,
        )
        .await
    }

    pub async fn get_unconfirmed_txs(&self, network: Network) -> DbResult<Vec<TransactionEntity>> {
        self.get_by_statuses(
            TransactionStatus::Sent,
            TransactionStatus::ErrorSent,
            TransactionStatus::Pending,
            network,
        )
        .await
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

    async fn get_by_statuses(
        &self,
        status1: TransactionStatus,
        status2: TransactionStatus,
        status3: TransactionStatus,
        network: Network,
    ) -> DbResult<Vec<TransactionEntity>> {
        readonly_transaction(self.pool, move |conn| {
            let txs: Vec<TransactionEntity> = dsl::transaction
                .filter(
                    (dsl::status
                        .eq(status1 as i32)
                        .or(dsl::status.eq(status2 as i32))
                        .or(dsl::status.eq(status3 as i32)))
                    .and(dsl::network.eq(network)),
                )
                .load(conn)?;
            Ok(txs)
        })
        .await
    }

    pub async fn update_tx_send_again(&self, tx_id: String, bump_gas: bool) -> DbResult<()> {
        let current_time = Utc::now().naive_utc();
        let new_status = match bump_gas {
            true => TransactionStatus::ResendAndBumpGas as i32,
            false => TransactionStatus::Resend as i32,
        };
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::transaction.find(tx_id))
                .set((
                    dsl::status.eq(new_status),
                    dsl::time_last_action.eq(current_time),
                    dsl::time_sent.eq::<Option<NaiveDateTime>>(None),
                ))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_tx_sent(
        &self,
        tx_id: String,
        tx_hash: String,
        gas_price: Option<String>,
    ) -> DbResult<()> {
        let current_time = Utc::now().naive_utc();
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::transaction.find(tx_id))
                .set((
                    dsl::status.eq(TransactionStatus::Sent as i32),
                    dsl::time_last_action.eq(current_time),
                    dsl::time_sent.eq(current_time),
                    dsl::tmp_onchain_txs.eq(tx_hash),
                    dsl::current_gas_price.eq(gas_price),
                ))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn confirm_tx(
        &self,
        tx_id: String,
        status: TransactionStatus,
        err: Option<String>,
        final_hash: Option<String>,
        final_gas_price: Option<String>,
    ) -> DbResult<()> {
        let current_time = Utc::now().naive_utc();
        let confirmed_time = current_time;
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::transaction.find(tx_id))
                .set((
                    dsl::status.eq(status as i32),
                    dsl::time_last_action.eq(current_time),
                    dsl::time_confirmed.eq(confirmed_time),
                    dsl::last_error_msg.eq(err),
                    dsl::current_gas_price.eq(final_gas_price),
                    dsl::final_tx.eq(final_hash),
                    dsl::tmp_onchain_txs.eq::<Option<String>>(None),
                    dsl::encoded.eq(""),
                    dsl::signature.eq::<Option<String>>(None),
                ))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_tx_status(
        &self,
        tx_id: String,
        status: TransactionStatus,
        err: Option<String>,
    ) -> DbResult<()> {
        let current_time = Utc::now().naive_utc();
        let confirmed_time = match status {
            TransactionStatus::Confirmed => Some(current_time),
            _ => None,
        };
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::transaction.find(tx_id))
                .set((
                    dsl::status.eq(status as i32),
                    dsl::time_last_action.eq(current_time),
                    dsl::time_confirmed.eq(confirmed_time),
                    dsl::last_error_msg.eq(err),
                ))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_tx_fields(
        &self,
        tx_id: String,
        encoded: String,
        signature: String,
        current_gas_price: Option<String>,
    ) -> DbResult<()> {
        let current_time = Utc::now().naive_utc();
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::transaction.find(tx_id))
                .set((
                    dsl::time_last_action.eq(current_time),
                    dsl::encoded.eq(encoded),
                    dsl::signature.eq(signature),
                    dsl::current_gas_price.eq(current_gas_price),
                ))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    //this is hacky solution for now to update db state so next check will properly resolve transaction status
    pub async fn overwrite_tmp_onchain_txs_and_status_back_to_pending(
        &self,
        tx_id: String,
        overwrite_tmp_onchain_txs: String,
    ) -> DbResult<()> {
        let current_time = Utc::now().naive_utc();
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::transaction.find(tx_id))
                .set((
                    dsl::time_last_action.eq(current_time),
                    dsl::tmp_onchain_txs.eq(overwrite_tmp_onchain_txs),
                    dsl::status.eq(TransactionStatus::Pending as i32),
                ))
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
