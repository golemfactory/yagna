/*
    Wallet functions on erc20.
*/

// External crates
use crate::erc20::ethereum::{
    get_polygon_gas_price_method, get_polygon_maximum_price, get_polygon_priority,
    get_polygon_starting_price, PolygonGasPriceMethod, PolygonPriority,
    POLYGON_PREFERRED_GAS_PRICES_EXPRESS, POLYGON_PREFERRED_GAS_PRICES_FAST,
    POLYGON_PREFERRED_GAS_PRICES_SLOW,
};
use bigdecimal::BigDecimal;
use chrono::Utc;
use num_bigint::BigUint;
use std::str::FromStr;
use web3::types::{H160, H256, U256, U64};

// Workspace uses
use ya_payment_driver::{
    db::models::{Network, TransactionEntity, TxType},
    model::{AccountMode, GenericError, Init, PaymentDetails},
};

// Local uses
use crate::erc20::transaction::YagnaRawTransaction;
use crate::{
    dao::Erc20Dao,
    erc20::{
        eth_utils, ethereum, faucet,
        utils::{
            big_dec_gwei_to_u256, big_dec_to_u256, big_uint_to_big_dec, convert_float_gas_to_u256,
            convert_u256_gas_to_float, str_to_addr, topic_to_str_address, u256_to_big_dec,
        },
    },
    RINKEBY_NETWORK,
};
use ya_payment_driver::db::models::TransactionStatus;

pub async fn account_balance(address: H160, network: Network) -> Result<BigDecimal, GenericError> {
    let balance_com = ethereum::get_glm_balance(address, network).await?;

    let balance = u256_to_big_dec(balance_com)?;
    log::debug!(
        "account_balance. address={}, network={}, balance={}",
        address,
        &network,
        &balance
    );

    Ok(balance)
}

pub async fn account_gas_balance(
    address: H160,
    network: Network,
) -> Result<BigDecimal, GenericError> {
    let balance_com = ethereum::get_balance(address, network).await?;
    let balance = u256_to_big_dec(balance_com)?;

    log::debug!(
        "account_gas_balance. address={}, network={}, balance={}",
        address,
        &network,
        &balance
    );

    Ok(balance)
}

pub async fn init_wallet(msg: &Init) -> Result<(), GenericError> {
    log::debug!("init_wallet. msg={:?}", msg);
    let mode = msg.mode();
    let address = msg.address();
    let network = msg.network().unwrap_or(RINKEBY_NETWORK.to_string());
    let network = Network::from_str(&network).map_err(|e| GenericError::new(e))?;

    if mode.contains(AccountMode::SEND) {
        let h160_addr = str_to_addr(&address)?;

        let glm_balance = ethereum::get_glm_balance(h160_addr, network).await?;
        if glm_balance == U256::zero() {
            return Err(GenericError::new("Insufficient GLM"));
        }

        let eth_balance = ethereum::get_balance(h160_addr, network).await?;
        if eth_balance == U256::zero() {
            return Err(GenericError::new("Insufficient ETH"));
        }
    }
    Ok(())
}

pub async fn fund(dao: &Erc20Dao, address: H160, network: Network) -> Result<(), GenericError> {
    if network == Network::Mainnet {
        return Err(GenericError::new("Wallet can not be funded on mainnet."));
    }
    faucet::request_glm(dao, address, network).await?;
    Ok(())
}

pub async fn get_next_nonce(
    dao: &Erc20Dao,
    address: H160,
    network: Network,
) -> Result<U256, GenericError> {
    let network_nonce = ethereum::get_next_nonce_pending(address, network).await?;
    let str_addr = format!("0x{:x}", &address);
    let db_nonce = dao.get_next_nonce(&str_addr, network).await?;

    if db_nonce > network_nonce {
        warn!(
            "Network nonce different than db nonce: {} != {}",
            network_nonce, db_nonce
        );
        return Ok(db_nonce);
    }

    Ok(network_nonce)
}

pub async fn has_enough_eth_for_gas(
    db_tx: &TransactionEntity,
    network: Network,
) -> Result<BigDecimal, GenericError> {
    let sender_h160 = str_to_addr(&db_tx.sender)?;
    let eth_balance = ethereum::get_balance(sender_h160, network).await?;
    let gas_costs = ethereum::get_max_gas_costs(db_tx)?;
    let gas_price = ethereum::get_gas_price_from_db_tx(db_tx)?;
    let human_gas_cost = u256_to_big_dec(gas_costs)?;
    let human_gas_price = convert_u256_gas_to_float(gas_price);
    if gas_costs > eth_balance {
        return Err(GenericError::new(format!(
            "Not enough ETH balance for gas. balance={}, gas_cost={}, gas_price={} Gwei, address={}, network={}",
            u256_to_big_dec(eth_balance)?,
            &human_gas_cost,
            &human_gas_price,
            &db_tx.sender,
            &db_tx.network
        )));
    }
    Ok(human_gas_cost)
}

pub async fn get_block_number(network: Network) -> Result<U64, GenericError> {
    ethereum::block_number(network).await
}

pub async fn make_transfer(
    details: &PaymentDetails,
    nonce: U256,
    network: Network,
    gas_price: Option<BigDecimal>,
    max_gas_price: Option<BigDecimal>,
    gas_limit: Option<u32>,
) -> Result<TransactionEntity, GenericError> {
    log::debug!(
        "make_transfer(). network={}, nonce={}, details={:?}",
        &network,
        &nonce,
        &details
    );
    let amount_big_dec = details.amount.clone();
    let amount = big_dec_to_u256(&amount_big_dec)?;

    let (gas_price, max_gas_price) = match network {
        Network::Polygon => match get_polygon_gas_price_method() {
            PolygonGasPriceMethod::PolygonGasPriceStatic => (
                Some(match gas_price {
                    Some(v) => big_dec_gwei_to_u256(v)?,
                    None => convert_float_gas_to_u256(get_polygon_starting_price()),
                }),
                Some(match max_gas_price {
                    Some(v) => big_dec_gwei_to_u256(v)?,
                    None => convert_float_gas_to_u256(get_polygon_maximum_price()),
                }),
            ),
            PolygonGasPriceMethod::PolygonGasPriceDynamic => (
                match gas_price {
                    None => None,
                    Some(v) => Some(big_dec_gwei_to_u256(v)?),
                },
                Some(match max_gas_price {
                    Some(v) => big_dec_gwei_to_u256(v)?,
                    None => convert_float_gas_to_u256(get_polygon_maximum_price()),
                }),
            ),
        },
        _ => (
            match gas_price {
                None => None,
                Some(v) => Some(big_dec_gwei_to_u256(v)?),
            },
            match max_gas_price {
                None => None,
                Some(v) => Some(big_dec_gwei_to_u256(v)?),
            },
        ),
    };

    let address = str_to_addr(&details.sender)?;
    let recipient = str_to_addr(&details.recipient)?;
    // TODO: Implement token
    //let token = get_network_token(network, None);
    let mut raw_tx = ethereum::prepare_raw_transaction(
        address, recipient, amount, network, nonce, gas_price, gas_limit,
    )
    .await?;

    if let Some(max_gas_price) = max_gas_price {
        if raw_tx.gas_price > max_gas_price {
            raw_tx.gas_price = max_gas_price;
        }
    }

    Ok(ethereum::create_dao_entity(
        nonce,
        address,
        raw_tx.gas_price.to_string(),
        max_gas_price.map(|v| v.to_string()),
        raw_tx.gas.as_u32() as i32,
        serde_json::to_string(&raw_tx).map_err(GenericError::new)?,
        network,
        Utc::now(),
        TxType::Transfer,
        Some(amount_big_dec),
    ))
}

fn bump_gas_price(gas_in_gwei: U256) -> U256 {
    let min_bump_num: U256 = U256::from(111u64);
    let min_bump_den: U256 = U256::from(100u64);
    let min_gas = gas_in_gwei * min_bump_num / min_bump_den;

    match get_polygon_gas_price_method() {
        PolygonGasPriceMethod::PolygonGasPriceDynamic => {
            //ignore maximum gas price, because we have to bump at least 10% so the transaction will be accepted
            min_gas
        }
        PolygonGasPriceMethod::PolygonGasPriceStatic => {
            let polygon_prices = get_polygon_priority();

            let gas_prices: &[f64] = match polygon_prices {
                PolygonPriority::PolygonPriorityExpress => {
                    &POLYGON_PREFERRED_GAS_PRICES_EXPRESS[..]
                }
                PolygonPriority::PolygonPriorityFast => &POLYGON_PREFERRED_GAS_PRICES_FAST[..],
                PolygonPriority::PolygonPrioritySlow => &POLYGON_PREFERRED_GAS_PRICES_SLOW[..],
            };

            gas_prices
                .iter()
                .map(|&f| convert_float_gas_to_u256(f))
                .find(|&gas_price_step| gas_price_step > min_gas)
                .unwrap_or(min_gas)
        }
    }
}

pub async fn send_transactions(
    dao: &Erc20Dao,
    txs: Vec<TransactionEntity>,
    network: Network,
) -> Result<(), GenericError> {
    // TODO: Use batch sending?
    for tx in txs {
        let mut raw_tx: YagnaRawTransaction =
            match serde_json::from_str::<YagnaRawTransaction>(&tx.encoded) {
                Ok(raw_tx) => raw_tx,
                Err(err) => {
                    log::error!(
                        "send_transactions - YagnaRawTransaction serialization failed: {:?}",
                        err
                    );
                    //handle problem when deserializing transaction
                    dao.transaction_confirmed_and_failed(
                        &tx.tx_id,
                        "",
                        None,
                        "Json parse failed, unrecoverable error",
                    )
                    .await;
                    continue;
                }
            };

        let address = str_to_addr(&tx.sender)?;

        let new_gas_price = if let Some(current_gas_price) = tx.current_gas_price {
            if tx.status == TransactionStatus::ResendAndBumpGas as i32 {
                let gas_u256 = U256::from_dec_str(&current_gas_price).map_err(GenericError::new)?;

                let max_gas_u256 = match tx.max_gas_price {
                    Some(max_gas_price) => {
                        Some(U256::from_dec_str(&max_gas_price).map_err(GenericError::new)?)
                    }
                    None => None,
                };
                let new_gas = bump_gas_price(gas_u256);
                if let Some(max_gas_u256) = max_gas_u256 {
                    if gas_u256 > max_gas_u256 {
                        log::warn!(
                            "bump gas ({}) larger than max gas ({}) price",
                            gas_u256,
                            max_gas_u256
                        )
                    }
                }
                new_gas
            } else {
                U256::from_dec_str(&current_gas_price).map_err(GenericError::new)?
            }
        } else if let Some(starting_gas_price) = tx.starting_gas_price {
            U256::from_dec_str(&starting_gas_price).map_err(GenericError::new)?
        } else {
            convert_float_gas_to_u256(get_polygon_starting_price())
        };
        raw_tx.gas_price = new_gas_price;

        let encoded = serde_json::to_string(&raw_tx).map_err(GenericError::new)?;
        let signature = ethereum::sign_raw_transfer_transaction(address, network, &raw_tx).await?;

        //save new parameters to db before proceeding. Maybe we should change status to sending
        dao.update_tx_fields(
            &tx.tx_id,
            encoded,
            hex::encode(&signature),
            Some(new_gas_price.to_string()),
        )
        .await;

        let signed = eth_utils::encode_signed_tx(&raw_tx, signature, network as u64);

        match ethereum::send_tx(signed, network).await {
            Ok(tx_hash) => {
                let str_tx_hash = format!("0x{:x}", &tx_hash);
                let str_tx_hash = if let Some(tmp_onchain_txs) = tx.tmp_onchain_txs {
                    tmp_onchain_txs + ";" + str_tx_hash.as_str()
                } else {
                    str_tx_hash
                };
                dao.transaction_sent(&tx.tx_id, &str_tx_hash, Some(raw_tx.gas_price.to_string()))
                    .await;
                log::info!("Send transaction. hash={}", &str_tx_hash);
                log::debug!("id={}", &tx.tx_id);
            }
            Err(e) => {
                log::error!("Error sending transaction: {:?}", e);
                if e.to_string().contains("nonce too low") {
                    if tx.tmp_onchain_txs.filter(|v| !v.is_empty()).is_some() && tx.resent_times < 5
                    {
                        //if tmp on-chain tx transactions exist give it a chance but marking it as failed sent
                        dao.transaction_failed_send(
                            &tx.tx_id,
                            tx.resent_times + 1,
                            e.to_string().as_str(),
                        )
                        .await;
                        continue;
                    } else {
                        //if trying to sent transaction too much times just end with unrecoverable error
                        log::error!("Nonce too low: {:?}", e);
                        dao.transaction_failed_with_nonce_too_low(
                            &tx.tx_id,
                            e.to_string().as_str(),
                        )
                        .await;
                        continue;
                    }
                }
                if e.to_string().contains("already known") {
                    log::error!("Already known: {:?}. Send transaction with higher gas to get from this error loop. (resent won't fix anything)", e);
                    dao.retry_send_transaction(&tx.tx_id, true).await;
                    continue;
                }

                dao.transaction_failed_send(&tx.tx_id, tx.resent_times, e.to_string().as_str())
                    .await;
            }
        }
    }
    Ok(())
}

// TODO: calculate fee. Below commented out reference to zkSync implementation
// pub async fn get_tx_fee(address: &str, network: Network) -> Result<BigDecimal, GenericError> {
//     // let token = get_network_token(network, None);
//     // let wallet = get_wallet(&address, network).await?;
//     // let tx_fee = wallet
//     //     .provider
//     //     .get_tx_fee(TxFeeTypes::Transfer, wallet.address(), token.as_str())
//     //     .await
//     //     .map_err(GenericError::new)?
//     //     .total_fee;
//     // let tx_fee_bigdec = utils::big_uint_to_big_dec(tx_fee);
//     //
//     // log::debug!("Transaction fee {:.5} {}", tx_fee_bigdec, token.as_str());
//     // Ok(tx_fee_bigdec)
//     todo!();
// }

pub async fn verify_tx(tx_hash: &str, network: Network) -> Result<PaymentDetails, GenericError> {
    log::debug!("verify_tx. hash={}", tx_hash);
    let hex_hash = H256::from_str(&tx_hash[2..]).unwrap();
    let tx = ethereum::get_tx_receipt(hex_hash, network).await?.unwrap();
    // TODO: Properly parse logs after https://github.com/tomusdrw/rust-web3/issues/208
    let tx_log = &tx.logs[0];
    let sender = topic_to_str_address(&tx_log.topics[1]);
    let recipient = topic_to_str_address(&tx_log.topics[2]);
    let amount = big_uint_to_big_dec(BigUint::from_bytes_be(&tx_log.data.0));
    // TODO: Get date from block
    // let date_str = format!("{}Z", v.created_at);
    let date = Some(chrono::Utc::now());

    let details = PaymentDetails {
        recipient,
        sender,
        amount,
        date,
    };
    log::debug!("PaymentDetails from server: {:?}", &details);

    Ok(details)
}
