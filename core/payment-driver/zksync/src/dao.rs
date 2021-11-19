/*
    Database Access Object, all you need to interact with the database.
*/

// Extrernal crates
use chrono::{DateTime, Utc};
use uuid::Uuid;

// Workspace uses
use ya_payment_driver::{
    dao::{payment::PaymentDao, transaction::TransactionDao, DbExecutor},
    db::models::{
        Network, PaymentEntity, TransactionEntity, TransactionStatus, TxType,
        PAYMENT_STATUS_FAILED, PAYMENT_STATUS_NOT_YET,
    },
    model::{GenericError, PaymentDetails, SchedulePayment},
    utils,
};

use crate::network::platform_to_network_token;

pub struct ZksyncDao {
    db: DbExecutor,
}

impl ZksyncDao {
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

    pub async fn get_accepted_payments(
        &self,
        node_id: &str,
        network: Network,
    ) -> Vec<PaymentEntity> {
        match self
            .payment()
            .get_accpeted_payments(node_id.to_string(), network)
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

    pub async fn insert_transaction(
        &self,
        details: &PaymentDetails,
        date: DateTime<Utc>,
        network: Network,
    ) -> String {
        // TO CHECK: No difference between tx_id and tx_hash on zksync
        // TODO: Implement pre-sign
        let tx_id = Uuid::new_v4().to_string();
        let tx = TransactionEntity {
            tx_id: tx_id.clone(),
            sender: details.sender.clone(),
            nonce: -1, // not used till pre-sign
            status: TransactionStatus::Created as i32,
            time_created: date.naive_utc(),
            time_last_action: date.naive_utc(),
            time_confirmed: None,
            time_sent: None,
            current_gas_price: None,
            starting_gas_price: None,
            max_gas_price: None,
            final_gas_used: None,
            amount_base: None,
            amount_erc20: None,
            gas_limit: None,
            tx_type: TxType::Transfer as i32, // Zksync only knows transfers, unused field
            encoded: "".to_string(),          // not used till pre-sign
            signature: None,                  // not used till pre-sign
            final_tx: None,
            tmp_onchain_txs: None,
            network,
            last_error_msg: None,
            resent_times: 0,
        };

        if let Err(e) = self.transaction().insert_transactions(vec![tx]).await {
            log::error!("Failed to store transaction for {:?} : {:?}", details, e)
            // TO CHECK: Should it continue or stop the process...
        }
        tx_id
    }

    pub async fn transaction_confirmed(&self, tx_id: &str) -> Vec<PaymentEntity> {
        if let Err(e) = self
            .transaction()
            .update_tx_status(tx_id.to_string(), TransactionStatus::Confirmed.into(), None)
            .await
        {
            log::error!("Failed to update tx status for {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
        match self.payment().get_by_tx_id(tx_id.to_string()).await {
            Ok(payments) => return payments,
            Err(e) => log::error!("Failed to fetch `payments` for tx {:?} : {:?}", tx_id, e),
        };
        vec![]
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

    pub async fn transaction_sent(&self, tx_id: &str, tx_hash: &str, order_id: &str) {
        if let Err(e) = self
            .payment()
            .update_tx_id(order_id.to_string(), tx_id.to_string())
            .await
        {
            log::error!("Failed to update for transaction {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
        if let Err(e) = self
            .transaction()
            .update_tx_sent(tx_id.to_string(), tx_hash.to_string(), None)
            .await
        {
            log::error!("Failed to update for transaction {:?} : {:?}", tx_id, e)
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn transaction_failed(&self, tx_id: &str, err: &str) {
        if let Err(e) = self
            .transaction()
            .update_tx_status(
                tx_id.to_string(),
                TransactionStatus::ErrorOnChain.into(),
                Some(err.to_string()),
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

    pub async fn retry_payment(&self, order_id: &str) {
        if let Err(e) = self
            .payment()
            .update_status(order_id.to_string(), PAYMENT_STATUS_NOT_YET)
            .await
        {
            log::error!(
                "Failed to set status of the `payment` {:?} to be retried : {:?}",
                order_id,
                e
            )
            // TO CHECK: Should it continue or stop the process...
        }
    }

    pub async fn get_unconfirmed_txs(&self, network: Network) -> Vec<TransactionEntity> {
        match self.transaction().get_unconfirmed_txs(network).await {
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
}
