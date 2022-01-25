/*
    Database Access Object, all you need to interact with the database.
*/

// Workspace uses
use ya_payment_driver::{
    dao::{payment::PaymentDao, transaction::TransactionDao, DbExecutor},
    db::models::{
        Network, PaymentEntity, TransactionEntity, TransactionStatus, PAYMENT_STATUS_FAILED,
        PAYMENT_STATUS_NOT_YET,
    },
    model::{GenericError, SchedulePayment},
    utils,
};

use crate::network::platform_to_network_token;

pub struct Erc20Dao {
    db: DbExecutor,
}

impl Erc20Dao {
    pub fn new(db: DbExecutor) -> Self {
        Self { db }
    }

    fn payment(&self) -> PaymentDao {
        self.db.as_dao::<PaymentDao>()
    }

    fn transaction(&self) -> TransactionDao {
        self.db.as_dao::<TransactionDao>()
    }

    pub async fn get_pending_payments(
        &self,
        node_id: &str,
        network: Network,
    ) -> Vec<PaymentEntity> {
        match self
            .payment()
            .get_pending_payments(node_id.to_string(), network)
            .await
        {
            Ok(payments) => payments,
            Err(e) => {
                log::error!(
                    "Failed to fetch pending payments for {:?} : {:?}",
                    node_id,
                    e
                );
                vec![]
            }
        }
    }

    pub async fn insert_payment(
        &self,
        order_id: &str,
        msg: &SchedulePayment,
    ) -> Result<(), GenericError> {
        let recipient = msg.recipient().to_owned();
        let glm_amount = utils::big_dec_to_u256(&msg.amount());
        let gas_amount = Default::default();
        let (network, _token) = platform_to_network_token(msg.platform())?;

        let payment = PaymentEntity {
            amount: utils::u256_to_big_endian_hex(glm_amount),
            gas: utils::u256_to_big_endian_hex(gas_amount),
            order_id: order_id.to_string(),
            payment_due_date: msg.due_date().naive_utc(),
            sender: msg.sender().clone(),
            recipient: recipient.clone(),
            status: PAYMENT_STATUS_NOT_YET,
            tx_id: None,
            network,
        };
        if let Err(e) = self.payment().insert(payment).await {
            log::error!(
                "Failed to store transaction for {:?} , msg={:?}, err={:?}",
                order_id,
                msg,
                e
            );
            return Err(GenericError::new(e));
        }
        Ok(())
    }

    pub async fn get_last_db_nonce_pending(
        &self,
        address: &str,
        network: Network,
    ) -> Result<Option<u64>, GenericError> {
        let res = self
            .transaction()
            .get_last_db_nonce(address, network)
            .await
            .map_err(GenericError::new)?;
        Ok(res.map(|val| val as u64))
    }

    pub async fn insert_raw_transaction(&self, tx: TransactionEntity) -> String {
        let tx_id = tx.tx_id.clone();

        if let Err(e) = self.transaction().insert_transactions(vec![tx]).await {
            log::error!("Failed to store transaction for {} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
        tx_id
    }

    pub async fn get_payments_based_on_tx(&self, tx_id: &str) -> Vec<PaymentEntity> {
        match self.payment().get_by_tx_id(tx_id.to_string()).await {
            Ok(payments) => payments,
            Err(e) => {
                log::error!("Failed to fetch `payments` for tx {:?} : {:?}", tx_id, e);
                vec![]
            }
        }
    }

    pub async fn transaction_confirmed(
        &self,
        tx_id: &str,
        final_hash: &str,
        final_gas_price: Option<String>,
    ) {
        if let Err(e) = self
            .transaction()
            .confirm_tx(
                tx_id.to_string(),
                TransactionStatus::Confirmed.into(),
                None,
                Some(final_hash.to_string()),
                final_gas_price,
            )
            .await
        {
            log::error!("Failed to update tx status for {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn transaction_confirmed_and_failed(
        &self,
        tx_id: &str,
        final_hash: &str,
        final_gas_price: Option<String>,
        error: &str,
    ) {
        if let Err(e) = self
            .transaction()
            .confirm_tx(
                tx_id.to_string(),
                TransactionStatus::ErrorOnChain.into(),
                Some(error.to_string()),
                Some(final_hash.to_string()),
                final_gas_price,
            )
            .await
        {
            log::error!("Failed to update tx status for {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn transaction_failed_with_nonce_too_low(&self, tx_id: &str, error: &str) {
        if let Err(e) = self
            .transaction()
            .confirm_tx(
                tx_id.to_string(),
                TransactionStatus::ErrorNonceTooLow.into(),
                Some(error.to_string()),
                None,
                None,
            )
            .await
        {
            log::error!("Failed to update tx status for {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn get_first_payment(&self, tx_hash: &str) -> Option<PaymentEntity> {
        match self
            .payment()
            .get_first_by_tx_hash(tx_hash.to_string())
            .await
        {
            Ok(payment) => Some(payment),
            Err(_) => None,
        }
    }

    pub async fn transaction_saved(&self, tx_id: &str, order_id: &str) {
        if let Err(e) = self
            .payment()
            .update_tx_id(order_id.to_string(), tx_id.to_string())
            .await
        {
            log::error!("Failed to update for transaction {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn retry_send_transaction(&self, tx_id: &str, bump_gas: bool) {
        if let Err(e) = self
            .transaction()
            .update_tx_send_again(tx_id.to_string(), bump_gas)
            .await
        {
            log::error!("Failed to update for transaction {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn update_tx_fields(
        &self,
        tx_id: &str,
        encoded: String,
        signature: String,
        current_gas_price: Option<String>,
    ) {
        if let Err(e) = self
            .transaction()
            .update_tx_fields(tx_id.to_string(), encoded, signature, current_gas_price)
            .await
        {
            log::error!("Failed to update for transaction {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn overwrite_tmp_onchain_txs_and_status_back_to_pending(
        &self,
        tx_id: &str,
        overwrite_tmp_onchain_txs: &str,
    ) {
        if let Err(e) = self
            .transaction()
            .overwrite_tmp_onchain_txs_and_status_back_to_pending(
                tx_id.to_string(),
                overwrite_tmp_onchain_txs.to_string(),
            )
            .await
        {
            log::error!("Failed to update for transaction {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn transaction_sent(&self, tx_id: &str, tx_hash: &str, gas_price: Option<String>) {
        if let Err(e) = self
            .transaction()
            .update_tx_sent(tx_id.to_string(), tx_hash.to_string(), gas_price)
            .await
        {
            log::error!("Failed to update for transaction {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn transaction_failed_send(&self, tx_id: &str, new_resent_count: i32, error: &str) {
        if let Err(e) = self
            .transaction()
            .update_error_sent(
                tx_id.to_string(),
                TransactionStatus::ErrorSent.into(),
                new_resent_count,
                Some(error.to_string()),
            )
            .await
        {
            log::error!(
                "Failed to update transaction failed in `transaction` {:?} : {:?}",
                tx_id,
                e
            )
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn payment_failed(&self, order_id: &str) {
        if let Err(e) = self
            .payment()
            .update_status(order_id.to_string(), PAYMENT_STATUS_FAILED)
            .await
        {
            log::error!(
                "Failed to update transaction failed in `payment` {:?} : {:?}",
                order_id,
                e
            )
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn get_unsent_txs(&self, network: Network, limit: i64) -> Vec<TransactionEntity> {
        match self.transaction().get_unsent_txs(network, limit).await {
            Ok(txs) => txs,
            Err(e) => {
                log::error!("Failed to fetch unconfirmed transactions : {:?}", e);
                vec![]
            }
        }
    }

    pub async fn get_unconfirmed_txs(&self, network: Network, limit: i64) -> Vec<TransactionEntity> {
        match self.transaction().get_unconfirmed_txs(network, limit).await {
            Ok(txs) => txs,
            Err(e) => {
                log::error!("Failed to fetch unconfirmed transactions : {:?}", e);
                vec![]
            }
        }
    }

    pub async fn has_unconfirmed_txs(&self) -> Result<bool, GenericError> {
        self.transaction()
            .has_unconfirmed_txs()
            .await
            .map_err(GenericError::new)
    }

    pub async fn get_pending_faucet_txs(
        &self,
        node_id: &str,
        network: Network,
    ) -> Vec<TransactionEntity> {
        match self
            .transaction()
            .get_pending_faucet_txs(node_id, network)
            .await
        {
            Ok(txs) => txs,
            Err(e) => {
                log::error!("Failed to fetch unsent transactions : {:?}", e);
                vec![]
            }
        }
    }
}
