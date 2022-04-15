/*
    Driver helper for handling timer events from a cron.
*/
// Extrnal crates
use chrono::{Duration, TimeZone, Utc};
use lazy_static::lazy_static;
use std::str::FromStr;
use web3::types::{H256, U256};

// Workspace uses
use ya_payment_driver::{
    bus,
    db::models::{Network, PaymentEntity, TransactionEntity, TxType},
    driver::BigDecimal,
    utils,
};

// Local uses
use crate::erc20::ethereum::get_env;
use crate::{
    dao::Erc20Dao,
    erc20::{ethereum, wallet},
    network,
};
use ya_payment_driver::db::models::TransactionStatus;
use ya_payment_driver::model::GenericError;

lazy_static! {
    static ref TX_SUMBIT_TIMEOUT: Duration = Duration::minutes(15);
    static ref ERC20_WAIT_FOR_TRANSACTION_ON_NETWORK: Duration = match std::env::var(
        "ERC20_WAIT_FOR_TRANSACTION_ON_NETWORK"
    )
    .map(|str| str.parse::<i64>())
    {
        Ok(Ok(seconds)) => Duration::seconds(seconds),
        _ => Duration::seconds(300),
    };
    static ref ERC20_WAIT_FOR_PENDING_ON_NETWORK: Duration =
        match std::env::var("ERC20_WAIT_FOR_PENDING_ON_NETWORK").map(|str| str.parse::<i64>()) {
            Ok(Ok(seconds)) => Duration::seconds(seconds),
            _ => Duration::seconds(600),
        };
    static ref ERC20_WAIT_FOR_ERROR_SENT_TRANSACTION: Duration = match std::env::var(
        "ERC20_WAIT_FOR_ERROR_SENT_TRANSACTION"
    )
    .map(|str| str.parse::<i64>())
    {
        Ok(Ok(seconds)) => Duration::seconds(seconds),
        _ => Duration::seconds(200),
    };
}

fn tmp_onchain_tx_to_vec(tmp_onchain_txs: Option<String>) -> Vec<String> {
    let tmp_onchain_txs = match tmp_onchain_txs {
        Some(tmp_onchain_txs) => tmp_onchain_txs,
        None => "".to_string(),
    };

    let mut tmp_onchain_txs_vec: Vec<String> = vec![];
    const EXPECTED_TRANSACTION_LENGTH: usize = 66;
    for str in tmp_onchain_txs.split(";") {
        if str.len() == EXPECTED_TRANSACTION_LENGTH && str.starts_with("0x") {
            //todo make proper validation of transaction hash
            tmp_onchain_txs_vec.push(str.to_string());
        }
    }
    tmp_onchain_txs_vec
}

async fn check_if_synched_and_get_block_number(network: Network) -> Result<u64, GenericError> {
    let env = get_env(network);

    // TODO: Store block number and continue only on new block
    let block_number = match wallet::get_block_number(network).await {
        Ok(block_number) => block_number.as_u64(),
        Err(err) => {
            return Err(GenericError::new(std::format!(
                "No block info can be downloaded, probably no connection to RPC: {:?}",
                err
            )));
        }
    };
    match ethereum::get_last_block_date(network, block_number).await {
        Ok(block_date) => {
            let current_time = Utc::now().naive_utc();
            let time_diff = current_time - block_date.naive_utc();
            if time_diff.num_seconds() < 0 {
                return Err(GenericError::new( "Last confirmed block time newer than current time, probably you system clock is setup wrongly"));
            }

            if time_diff.num_seconds() > env.latest_block_max_seconds_behind {
                return Err(GenericError::new(std::format!(
                    "RPC node {} is not synced. Last block is {} seconds behind. Last synched block {}",
                    network,
                    time_diff.num_seconds(),
                    block_number
                )));
            }
        }
        Err(err) => {
            return Err(GenericError::new(std::format!(
                "Last block info cannot be downloaded: {:?}",
                err
            )));
        }
    };
    Ok(block_number)
}

pub async fn confirm_payments(
    dao: &Erc20Dao,
    name: &str,
    network_key: &str,
) -> Result<bool, GenericError> {
    let network = Network::from_str(&network_key).unwrap();
    let env = get_env(network);
    let txs = dao
        .get_unconfirmed_txs(network, env.payment_max_processed as i64)
        .await;
    //log::debug!("confirm_payments {:?}", txs);

    if txs.is_empty() {
        Ok(true)
    } else {
        // TODO: Store block number and continue only on new block
        let block_number = match check_if_synched_and_get_block_number(network).await {
            Ok(block_number) => block_number,
            Err(err) => {
                //log::error!("{:?}", err);
                return Err(err);
            }
        };
        let current_time = Utc::now().naive_utc();

        'main_tx_loop: for tx in txs {
            log::debug!("checking tx {:?}", &tx);

            let time_elapsed_from_sent = tx.time_sent.map(|ts| current_time - ts);

            let time_elapsed_from_last_action = current_time - tx.time_last_action;

            let tmp_onchain_txs_vec = tmp_onchain_tx_to_vec(tx.tmp_onchain_txs);

            if tx.status == TransactionStatus::ErrorSent as i32 {
                //try to check older transactions for success
                //the error should be nonce too low - maybe we should check for this here
                for existing_tx_hash in &tmp_onchain_txs_vec {
                    //ignore malformed strings
                    let hex_hash = match H256::from_str(&existing_tx_hash[2..]) {
                        Ok(hex_hash) => hex_hash,
                        Err(err) => {
                            log::error!("Error when getting transaction hex hash: {:?}", err);
                            continue;
                        }
                    };
                    let tcs =
                        match ethereum::get_tx_on_chain_status(hex_hash, block_number, network)
                            .await
                        {
                            Ok(tcs) => tcs,
                            Err(err) => {
                                log::error!("Error when getting get_tx_on_chain_status: {:?}", err);
                                continue;
                            }
                        };
                    if tcs.exists_on_chain && !tcs.pending {
                        log::debug!("Previously sent transaction confirmed");
                        dao.overwrite_tmp_onchain_txs_and_status_back_to_pending(
                            &tx.tx_id,
                            existing_tx_hash,
                        )
                        .await;
                        continue 'main_tx_loop;
                    }
                }
            }
            if tx.status == TransactionStatus::ErrorSent as i32 {
                if time_elapsed_from_last_action > *ERC20_WAIT_FOR_ERROR_SENT_TRANSACTION {
                    log::warn!(
                        "Transaction {} not found on chain for {:?}",
                        tx.tx_id,
                        time_elapsed_from_sent
                    );
                    log::warn!("Time since last action {:?}", time_elapsed_from_last_action);
                    dao.retry_send_transaction(&tx.tx_id, false).await;
                    continue;
                }
            }

            if tmp_onchain_txs_vec.len() == 0 {
                continue;
            }

            let newest_tx = match tmp_onchain_txs_vec.last() {
                Some(last_el) => last_el.as_str(),
                None => {
                    log::error!("Error when getting last onchain tx from db");
                    continue;
                }
            };

            log::debug!(
                "Checking if tx was a success. network={}, block={}, hash={}",
                &network,
                block_number,
                &newest_tx
            );

            let tx_hex_hash = match H256::from_str(&newest_tx[2..]) {
                Ok(hex_hash) => hex_hash,
                Err(err) => {
                    log::error!("Error when getting transaction hex hash: {:?}", err);
                    continue;
                }
            };
            let transaction_chain_status =
                match ethereum::get_tx_on_chain_status(tx_hex_hash, block_number, network).await {
                    Ok(hex_hash) => hex_hash,
                    Err(err) => {
                        log::error!("Error when getting get_tx_on_chain_status: {:?}", err);
                        continue;
                    }
                };

            let final_gas_price = match transaction_chain_status.gas_price {
                Some(gas_price) => Some(gas_price.to_string()),
                None => None,
            };

            if !transaction_chain_status.exists_on_chain {
                log::warn!("Transaction not found on chain");
                if time_elapsed_from_last_action > *ERC20_WAIT_FOR_TRANSACTION_ON_NETWORK {
                    log::warn!(
                        "Transaction {} not found on chain for {:?}",
                        tx.tx_id,
                        time_elapsed_from_sent
                    );
                    log::warn!("Time since last action {:?}", time_elapsed_from_last_action);
                    dao.retry_send_transaction(&tx.tx_id, false).await;
                }

                continue;
            } else if transaction_chain_status.pending {
                if time_elapsed_from_last_action > *ERC20_WAIT_FOR_PENDING_ON_NETWORK {
                    let cur_gas_price = tx
                        .current_gas_price
                        .and_then(|str| U256::from_dec_str(&str).ok())
                        .unwrap_or_default();

                    let max_gas_price = tx
                        .max_gas_price
                        .and_then(|str| U256::from_dec_str(&str).ok())
                        .unwrap_or_default();

                    if cur_gas_price.is_zero() || max_gas_price.is_zero() {
                        log::debug!(
                            "Wrong gas prices: cur_gas_price: {} max_gas_price: {}",
                            cur_gas_price,
                            max_gas_price
                        );
                        continue;
                    }
                    if cur_gas_price >= max_gas_price {
                        log::debug!("Cannot bump gas more: Current gas price current_gas_price: {} max_gas_price: {}", cur_gas_price, max_gas_price);
                        continue;
                    }

                    log::warn!(
                        "Transaction not found on chain for {:?}",
                        time_elapsed_from_sent
                    );
                    log::warn!("Time since last action {:?}", time_elapsed_from_last_action);
                    dao.retry_send_transaction(&tx.tx_id, true).await;
                }

                continue;
            } else if !transaction_chain_status.confirmed {
                log::debug!("Transaction is commited, but we are waiting for confirmations");
                continue;
            } else if transaction_chain_status.succeeded {
                log::info!("Transaction confirmed and succeeded");

                dao.transaction_confirmed(&tx.tx_id, &newest_tx, final_gas_price)
                    .await;
                // Faucet can stop here IF the tx was a success.
                if tx.tx_type == TxType::Faucet as i32 {
                    log::debug!("Faucet tx confirmed, exit early. hash={}", &newest_tx);
                    continue;
                }
                if tx.tx_type == TxType::Approve as i32 {
                    log::debug!("Approve tx confirmed, exit early. hash={}", &newest_tx);
                    continue;
                }

                let payments = dao.get_payments_based_on_tx(&tx.tx_id).await;

                // CLI Transfer ( no related payments ) can stop here IF the tx was a success.
                if tx.tx_type == TxType::Transfer as i32 && payments.is_empty() {
                    log::debug!("Transfer confirmed, exit early. hash={}", &newest_tx);
                    continue;
                }
                let order_ids: Vec<String> = payments
                    .iter()
                    .map(|payment| payment.order_id.clone())
                    .collect();

                let platform = match network::network_token_to_platform(Some(network), None) {
                    Ok(platform) => platform,
                    Err(e) => {
                        log::error!(
                            "Error when converting network_token_to_platform. hash={}. Err={:?}",
                            &newest_tx,
                            e
                        );
                        continue;
                    }
                };
                let details = match wallet::verify_tx(&newest_tx, network).await {
                    Ok(a) => a,
                    Err(e) => {
                        log::warn!("Failed to get transaction details from erc20, creating bespoke details. Error={}", e);

                        let first_payment: PaymentEntity =
                            match dao.get_first_payment(&newest_tx).await {
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

                let newest_tx = hex::decode(&newest_tx[2..]).unwrap();
                if let Err(e) =
                    bus::notify_payment(name, &platform, order_ids, &details, newest_tx).await
                {
                    log::error!("{}", e)
                };
            } else {
                log::info!("Transaction confirmed, but resulted in error");

                dao.transaction_confirmed_and_failed(
                    &tx.tx_id,
                    &newest_tx,
                    final_gas_price,
                    "Failure on chain during execution",
                )
                .await;

                let payments = dao.get_payments_based_on_tx(&tx.tx_id).await;

                let order_ids: Vec<String> = payments
                    .iter()
                    .map(|payment| payment.order_id.clone())
                    .collect();
                for order_id in order_ids.iter() {
                    dao.payment_failed(order_id).await;
                }
                continue;
            }
        }
        Ok(false)
    }
}

pub async fn process_payments_for_account(
    dao: &Erc20Dao,
    node_id: &str,
    network: Network,
) -> Result<(), GenericError> {
    log::trace!(
        "Processing payments for node_id={}, network={}",
        node_id,
        network
    );
    let payments: Vec<PaymentEntity> = dao.get_pending_payments(node_id, network).await;
    if !payments.is_empty() {
        let _block_number = match check_if_synched_and_get_block_number(network).await {
            Ok(block_number) => block_number,
            Err(err) => {
                log::error!("{:?}", err);
                return Ok(());
            }
        };

        log::info!(
            "Processing payments. count={}, network={} node_id={}",
            payments.len(),
            network,
            node_id
        );
        let next_nonce_info =
            wallet::get_next_nonce_info(dao, crate::erc20::utils::str_to_addr(&node_id)?, network)
                .await?;

        let mut next_nonce = if let Some(db_nonce_pending) = next_nonce_info.db_nonce_pending {
            db_nonce_pending
        } else {
            next_nonce_info.network_nonce_latest
        };

        log::warn!("Payments: nonce_info={:?}", &next_nonce_info);
        let env = ethereum::get_env(network);
        log::warn!("Max processed {}", env.payment_max_processed);

        let multi_batching = true;

        if multi_batching {
            if payments.len() >= 1 {
                handle_multi_payment(&dao, payments, &mut next_nonce).await?;
            }
        } else {
            for payment in payments {
                if next_nonce >= next_nonce_info.network_nonce_latest + env.payment_max_processed {
                    break;
                }
                handle_payment(&dao, payment, &mut next_nonce).await?;
            }
        }
    }
    Ok(())
}

pub async fn process_transactions(dao: &Erc20Dao, network: Network) {
    let env = get_env(network);
    let transactions: Vec<TransactionEntity> = dao
        .get_unsent_txs(network, (env.payment_max_processed) as i64)
        .await;
    //todo get proper nonce
    if !transactions.is_empty() {
        log::debug!("transactions: {:?}", transactions);

        let _block_number = match check_if_synched_and_get_block_number(network).await {
            Ok(block_number) => block_number,
            Err(err) => {
                log::error!("{:?}", err);
                return;
            }
        };

        match wallet::send_transactions(dao, transactions, network).await {
            Ok(()) => log::debug!("transactions sent!"),
            Err(e) => log::error!("transactions sent ERROR: {:?}", e),
        };
    }
}

async fn handle_multi_payment(
    dao: &Erc20Dao,
    payments: Vec<PaymentEntity>,
    nonce: &mut u64,
) -> Result<(), GenericError> {
    let details_array = payments
        .iter()
        .map(|payment| utils::db_to_payment_details(&payment))
        .collect();

    let tx_nonce = nonce.to_owned();

    let network = payments
        .get(0)
        .ok_or(GenericError::new("TODO: Empty array"))?
        .network;

    match wallet::make_multi_transfer(details_array, tx_nonce, network, None, None, None).await {
        Ok(db_tx) => {
            let tx_id = dao
                .insert_raw_transaction(db_tx)
                .await
                .map_err(GenericError::new)?;
            for payment in payments {
                dao.transaction_saved(&tx_id, &payment.order_id).await;
            }
            *nonce += 1;
        }
        Err(e) => {
            log::error!("Failed to send multipayments {:?}", e);
            /*let deadline = Utc.from_utc_datetime(&payment.payment_due_date) + *TX_SUMBIT_TIMEOUT;
            if Utc::now() > deadline {
                log::error!("Failed to submit erc20 transaction. Retry deadline reached. details={:?} error={}", payment, e);
                for payment in payments {
                    dao.payment_failed(&payment.order_id).await;
                }
            } else {
                log::warn!(
                    "Failed to submit erc20 transaction. Payment will be retried until {}. details={:?} error={:?}",
                    deadline, payment, e
                );
            };*/
        }
    };
    Ok(())
}

async fn handle_payment(
    dao: &Erc20Dao,
    payment: PaymentEntity,
    nonce: &mut u64,
) -> Result<(), GenericError> {
    let details = utils::db_to_payment_details(&payment);
    let tx_nonce = nonce.to_owned();

    match wallet::make_transfer(&details, tx_nonce, payment.network, None, None, None).await {
        Ok(db_tx) => {
            let tx_id = dao
                .insert_raw_transaction(db_tx)
                .await
                .map_err(GenericError::new)?;
            dao.transaction_saved(&tx_id, &payment.order_id).await;
            *nonce += 1;
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
    Ok(())
}
