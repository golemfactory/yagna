/*
    Wallet functions on erc20.
*/

// External crates
use bigdecimal::BigDecimal;
use num_bigint::BigUint;
use std::str::FromStr;
use web3::types::{H160, H256, U256, U64};

// Workspace uses
use ya_payment_driver::{
    db::models::{Network, TransactionEntity},
    model::{AccountMode, GenericError, Init, PaymentDetails},
};

// Local uses
use crate::{
    dao::Erc20Dao,
    erc20::{eth_utils, ethereum, faucet, utils},
    DEFAULT_NETWORK,
};

pub async fn account_balance(address: H160, network: Network) -> Result<BigDecimal, GenericError> {
    let balance_com = ethereum::get_glm_balance(address, network).await?;

    let balance = utils::u256_to_big_dec(balance_com)?;
    log::debug!(
        "account_balance. address={}, network={}, balance={}",
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
    let network = msg.network().unwrap_or(DEFAULT_NETWORK.to_string());
    let network = Network::from_str(&network).map_err(|e| GenericError::new(e))?;

    if mode.contains(AccountMode::SEND) {
        let h160_addr = utils::str_to_addr(&address)?;

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

pub async fn get_network_nonce(address: H160, network: Network) -> Result<U256, GenericError> {
    ethereum::get_next_nonce(address, network).await
}

pub async fn get_block_number(network: Network) -> Result<U64, GenericError> {
    ethereum::block_number(network).await
}

pub async fn make_transfer(
    details: &PaymentDetails,
    nonce: U256,
    network: Network,
) -> Result<TransactionEntity, GenericError> {
    log::debug!(
        "make_transfer(). network={}, nonce={}, details={:?}",
        &network,
        &nonce,
        &details
    );
    let amount = details.amount.clone();
    let amount = utils::big_dec_to_u256(amount)?;

    let address = utils::str_to_addr(&details.sender)?;
    let recipient = utils::str_to_addr(&details.recipient)?;
    // TODO: Implement token
    //let token = get_network_token(network, None);
    ethereum::sign_transfer_tx(address, recipient, amount, network, nonce).await
}

pub async fn send_transactions(
    dao: &Erc20Dao,
    txs: Vec<TransactionEntity>,
    network: Network,
) -> Result<(), GenericError> {
    // TODO: Use batch sending?
    for tx in txs {
        let raw_tx = serde_json::from_str(&tx.encoded).map_err(GenericError::new)?;
        let sign = hex::decode(tx.signature).map_err(GenericError::new)?;
        let signed = eth_utils::encode_signed_tx(&raw_tx, sign, network as u64);

        match ethereum::send_tx(signed, network).await {
            Ok(tx_hash) => {
                let str_tx_hash = format!("0x{:x}", &tx_hash);
                dao.transaction_sent(&tx.tx_id, &str_tx_hash).await;
                log::info!("Send transaction. hash={}", &str_tx_hash);
                log::debug!("id={}", &tx.tx_id);
            }
            Err(e) => {
                log::error!("Error sending transaction: {:?}", e);
                dao.transaction_failed(&tx.tx_id).await;
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

pub async fn check_tx(
    tx_hash: &str,
    block_number: &U64,
    network: Network,
) -> Option<Result<(), String>> {
    let hex_hash = H256::from_str(&tx_hash[2..]).unwrap();
    match ethereum::is_tx_confirmed(hex_hash, block_number, network).await {
        Ok(false) => None,
        Ok(true) => Some(Ok(())),
        Err(e) => Some(Err(format!("check_tx ERROR: {:?}", e))),
    }
}

pub async fn verify_tx(tx_hash: &str, network: Network) -> Result<PaymentDetails, GenericError> {
    log::debug!("verify_tx. hash={}", tx_hash);
    let hex_hash = H256::from_str(&tx_hash[2..]).unwrap();
    let tx = ethereum::get_tx_receipt(hex_hash, network).await?.unwrap();

    // TODO: Properly parse logs after https://github.com/tomusdrw/rust-web3/issues/208
    let tx_log = &tx.logs[0];
    let sender = utils::topic_to_str_address(&tx_log.topics[1]);
    let recipient = utils::topic_to_str_address(&tx_log.topics[2]);
    let amount = utils::big_uint_to_big_dec(BigUint::from_bytes_be(&tx_log.data.0));
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
