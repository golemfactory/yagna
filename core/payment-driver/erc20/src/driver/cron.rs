/*
    Driver helper for handling timer events from a cron.
*/
// Extrnal crates
use chrono::{Duration, TimeZone, Utc};
use lazy_static::lazy_static;
use std::str::FromStr;
use web3::types::U256;
// Workspace uses
use ya_payment_driver::{
    bus,
    db::models::{Network, PaymentEntity, TransactionEntity, TxType},
    driver::BigDecimal,
    utils,
};

// Local uses
use crate::{dao::Erc20Dao, erc20::wallet, network};

lazy_static! {
    static ref TX_SUMBIT_TIMEOUT: Duration = Duration::minutes(15);
}

pub async fn confirm_payments(dao: &Erc20Dao, name: &str, network_key: &str) {
    let network = Network::from_str(&network_key).unwrap();
    let txs = dao.get_unconfirmed_txs(network).await;
    log::trace!("confirm_payments {:?}", txs);

    if !txs.is_empty() {
        // TODO: Store block number and continue only on new block
        let block_number = wallet::get_block_number(network).await.unwrap();

        for tx in txs {
            log::trace!("checking tx {:?}", &tx);
            let tx_hash = match &tx.tx_hash {
                None => continue,
                Some(tx_hash) => tx_hash,
            };
            log::debug!(
                "Checking if tx was a success. network={}, block={}, hash={}",
                &network,
                &block_number,
                &tx_hash
            );
            let tx_success = match wallet::check_tx(&tx_hash, &block_number, network).await {
                None => continue, // Check_tx returns None when the result is unknown
                Some(tx_success) => tx_success,
            };

            let payments = dao.transaction_confirmed(&tx.tx_id).await;
            // Faucet can stop here IF the tx was a success.
            if tx.tx_type == TxType::Faucet as i32 && tx_success.is_ok() {
                log::debug!("Faucet tx confirmed, exit early. hash={}", &tx_hash);
                continue;
            }
            let order_ids: Vec<String> = payments
                .iter()
                .map(|payment| payment.order_id.clone())
                .collect();

            if let Err(err) = tx_success {
                log::error!(
                    "ZkSync transaction verification failed. tx_details={:?} error={}",
                    tx,
                    err
                );
                dao.transaction_failed(&tx.tx_id).await;
                for order_id in order_ids.iter() {
                    dao.payment_failed(order_id).await;
                }
                continue;
            }

            // TODO: Add token support
            let platform = network::network_token_to_platform(Some(network), None).unwrap(); // TODO: Catch error?
            let details = match wallet::verify_tx(&tx_hash, network).await {
                Ok(a) => a,
                Err(e) => {
                    log::warn!("Failed to get transaction details from erc20, creating bespoke details. Error={}", e);

                    let first_payment: PaymentEntity = match dao.get_first_payment(&tx_hash).await {
                        Some(p) => p,
                        None => continue,
                    };

                    //Create bespoke payment details:
                    // - Sender + receiver are the same
                    // - Date is always now
                    // - Amount needs to be updated to total of all PaymentEntity's
                    let mut details = utils::db_to_payment_details(&first_payment);
                    details.amount = payments
                        .into_iter()
                        .map(|payment| utils::db_amount_to_big_dec(payment.amount.clone()))
                        .sum::<BigDecimal>();
                    details
                }
            };

            let tx_hash = hex::decode(&tx_hash[2..]).unwrap();
            if let Err(e) = bus::notify_payment(name, &platform, order_ids, &details, tx_hash).await
            {
                log::error!("{}", e)
            };
        }
    }
}

pub async fn process_payments_for_account(dao: &Erc20Dao, node_id: &str, network: Network) {
    log::trace!(
        "Processing payments for node_id={}, network={}",
        node_id,
        network
    );
    let payments: Vec<PaymentEntity> = dao.get_pending_payments(node_id, network).await;
    if !payments.is_empty() {
        log::info!(
            "Processing payments. count={}, network={} node_id={}",
            payments.len(),
            network,
            node_id
        );
        let network_nonce =
            wallet::get_network_nonce(crate::erc20::utils::str_to_addr(&node_id).unwrap(), network)
                .await
                .unwrap();
        let db_nonce = dao.get_next_nonce(&node_id, network).await.unwrap();
        let mut nonce = std::cmp::max(network_nonce, db_nonce);
        log::debug!("Payments: nonce={}, details={:?}", &nonce, payments);
        for payment in payments {
            handle_payment(&dao, payment, &mut nonce).await;
        }
    }
}

pub async fn process_transactions(dao: &Erc20Dao, network: Network) {
    let transactions: Vec<TransactionEntity> = dao.get_unsent_txs(network).await;

    if !transactions.is_empty() {
        log::debug!("transactions: {:?}", transactions);
        match wallet::send_transactions(dao, transactions, network).await {
            Ok(()) => log::debug!("transactions sent!"),
            Err(e) => log::error!("transactions sent ERROR: {:?}", e),
        };
    }
}

async fn handle_payment(dao: &Erc20Dao, payment: PaymentEntity, nonce: &mut U256) {
    let details = utils::db_to_payment_details(&payment);
    let tx_nonce = nonce.to_owned();

    match wallet::make_transfer(&details, tx_nonce, payment.network).await {
        Ok(db_tx) => {
            let tx_id = dao.insert_raw_transaction(db_tx).await;
            dao.transaction_saved(&tx_id, &payment.order_id).await;
            *nonce += U256::from(1);
        }
        Err(e) => {
            let deadline = Utc.from_utc_datetime(&payment.payment_due_date) + *TX_SUMBIT_TIMEOUT;
            if Utc::now() > deadline {
                log::error!("Failed to submit erc20 transaction. Retry deadline reached. details={:?} error={}", payment, e);
                dao.payment_failed(&payment.order_id).await;
            } else {
                log::warn!(
                    "Failed to submit erc20 transaction. Payment will be retried until {}. details={:?} error={}",
                    deadline, payment, e
                );
            };
        }
    };
}
