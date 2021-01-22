/*
    Wallet functions on zksync.
*/

// External crates
use bigdecimal::{BigDecimal, Zero};
use num::BigUint;
use std::env;
use std::str::FromStr;
use zksync::operations::SyncTransactionHandle;
use zksync::types::BlockStatus;
use zksync::zksync_types::{tx::TxHash, Address, TxFeeTypes};
use zksync::{Network as ZkNetwork, Provider, Wallet, WalletCredentials};
use zksync_eth_signer::EthereumSigner;

// Workspace uses
use ya_payment_driver::{
    db::models::Network,
    model::{AccountMode, Exit, GenericError, Init, PaymentDetails},
};

// Local uses
use crate::{
    network::get_network_token,
    zksync::{faucet, signer::YagnaEthSigner, utils},
    DEFAULT_NETWORK, ZKSYNC_TOKEN_NAME,
};

pub async fn account_balance(address: &str, network: Network) -> Result<BigDecimal, GenericError> {
    let pub_address = Address::from_str(&address[2..]).map_err(GenericError::new)?;
    let acc_info = get_provider(network)
        .account_info(pub_address)
        .await
        .map_err(GenericError::new)?;
    // TODO: implement tokens, replace None
    let token = get_network_token(network, None);
    let mut balance_com = acc_info
        .committed
        .balances
        .get(&token)
        .map(|x| x.0.clone())
        .unwrap_or(BigUint::zero());
    // Hack to get GNT balance for backwards compatability
    // TODO: Remove this if {} and the `mut` from `let mut balance_com`
    if network == Network::Rinkeby && balance_com == BigUint::zero() {
        balance_com = acc_info
            .committed
            .balances
            .get(ZKSYNC_TOKEN_NAME)
            .map(|x| x.0.clone())
            .unwrap_or(BigUint::zero());
    }
    let balance = utils::big_uint_to_big_dec(balance_com);
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
    let address = msg.address().clone();
    let network = msg.network().unwrap_or(DEFAULT_NETWORK.to_string());
    let network = Network::from_str(&network).map_err(|e| GenericError::new(e))?;

    if mode.contains(AccountMode::SEND) {
        if network != Network::Mainnet {
            faucet::request_ngnt(&address, network).await?;
        }
        let wallet = get_wallet(&address, network).await?;
        unlock_wallet(wallet, network).await?;
    }
    Ok(())
}

pub async fn exit(msg: &Exit) -> Result<String, GenericError> {
    let network = msg.network().unwrap_or(DEFAULT_NETWORK.to_string());
    let network = Network::from_str(&network).map_err(|e| GenericError::new(e))?;
    let wallet = get_wallet(&msg.sender(), network).await?;
    let tx_handle = withdraw(wallet, msg.amount(), msg.to()).await?;
    let tx_info = tx_handle
        .wait_for_commit()
        .await
        .map_err(GenericError::new)?;

    match tx_info.success {
        Some(true) => Ok(hash_to_hex(tx_handle.hash())),
        Some(false) => Err(GenericError::new(
            tx_info
                .fail_reason
                .unwrap_or("Unknown failure reason".to_string()),
        )),
        None => Err(GenericError::new("Transaction time-outed")),
    }
}

fn hash_to_hex(hash: TxHash) -> String {
    // TxHash::to_string adds a prefix to the hex value
    hex::encode(hash.as_ref())
}

pub async fn get_nonce(address: &str, network: Network) -> u32 {
    let addr = match Address::from_str(&address[2..]) {
        Ok(a) => a,
        Err(e) => {
            log::error!("Unable to parse address, failed to get nonce. {:?}", e);
            return 0;
        }
    };
    let provider = get_provider(network);
    let account_info = match provider.account_info(addr).await {
        Ok(i) => i,
        Err(e) => {
            log::error!("Unable to get account info, failed to get nonce. {:?}", e);
            return 0;
        }
    };
    account_info.committed.nonce
}

pub async fn make_transfer(
    details: &PaymentDetails,
    nonce: u32,
    network: Network,
) -> Result<String, GenericError> {
    log::debug!("make_transfer. {:?}", details);
    let amount = details.amount.clone();
    let amount = utils::big_dec_to_big_uint(amount)?;
    let amount = utils::pack_up(&amount);

    let sender = details.sender.clone();
    let wallet = get_wallet(&sender, network).await?;
    let mut token = get_network_token(network, None);
    // TODO Investiggate and fix tGLM ticker name on rinkeby
    if token == "tGLM" {
        token = ZKSYNC_TOKEN_NAME.to_string();
    }

    let balance = wallet
        .get_balance(BlockStatus::Committed, token.as_ref())
        .await
        .map_err(GenericError::new)?;
    log::debug!("balance before transfer={}", balance);

    let transfer = wallet
        .start_transfer()
        .nonce(nonce)
        .str_to(&details.recipient[2..])
        .map_err(GenericError::new)?
        .token(token.as_ref()) // TODO: use `token`
        .map_err(GenericError::new)?
        .amount(amount)
        .send()
        .await
        .map_err(GenericError::new)?;

    let tx_hash = hex::encode(transfer.hash());
    log::info!("Created zksync transaction with hash={}", tx_hash);
    Ok(tx_hash)
}

pub async fn check_tx(tx_hash: &str, network: Network) -> Option<bool> {
    let provider = get_provider(network);
    let tx_hash = format!("sync-tx:{}", tx_hash);
    let tx_hash = TxHash::from_str(&tx_hash).unwrap();
    let tx_info = provider.tx_info(tx_hash).await.unwrap();
    log::trace!("tx_info: {:?}", tx_info);
    tx_info.success
}
//  TODO: Get Transfer object from zksync
// pub async fn build_payment_details(tx_hash: &str) -> PaymentDetails {
//     let provider = get_provider();
//     let tx_hash = format!("sync-tx:{}", tx_hash);
//     let tx_hash = TxHash::from_str(&tx_hash).unwrap();
//     let tx_info = provider.tx_info(tx_hash).await.unwrap();
//
//     PaymentDetails {
//         recipient: tx_info.,
//         sender,
//         amount,
//         date
//     }
// }

fn get_provider(network: Network) -> Provider {
    let provider: Provider = match env::var("ZKSYNC_RPC_ADDRESS").ok() {
        Some(rpc_addr) => Provider::from_addr(rpc_addr),
        None => Provider::new(get_zk_network(network)),
    };
    provider.clone()
}

async fn get_wallet(
    address: &str,
    network: Network,
) -> Result<Wallet<YagnaEthSigner>, GenericError> {
    log::debug!("get_wallet {:?}", address);
    let addr = Address::from_str(&address[2..]).map_err(GenericError::new)?;
    let provider = get_provider(network);
    let signer = YagnaEthSigner::new(addr);
    let credentials = WalletCredentials::from_eth_signer(addr, signer, get_zk_network(network))
        .await
        .map_err(GenericError::new)?;
    let wallet = Wallet::new(provider, credentials)
        .await
        .map_err(GenericError::new)?;
    Ok(wallet)
}

fn get_zk_network(network: Network) -> ZkNetwork {
    ZkNetwork::from_str(&network.to_string()).unwrap() // _or(ZkNetwork::Rinkeby)
}

async fn unlock_wallet<S: EthereumSigner + Clone>(
    wallet: Wallet<S>,
    network: Network,
) -> Result<(), GenericError> {
    log::debug!("unlock_wallet");
    if !wallet
        .is_signing_key_set()
        .await
        .map_err(GenericError::new)?
    {
        log::info!("Unlocking wallet... address = {}", wallet.signer.address);
        let mut token = get_network_token(network, None);
        // TODO Investiggate and fix tGLM ticker name on rinkeby
        if token == "tGLM" {
            token = ZKSYNC_TOKEN_NAME.to_string();
        }
        let unlock = wallet
            .start_change_pubkey()
            .fee_token(token.as_ref())
            .map_err(|e| GenericError::new(format!("Failed to create change_pubkey request: {}", e)))?
            .send()
            .await
            .map_err(|e| GenericError::new(format!("Failed to send change_pubkey request: '{}'. HINT: Did you run `yagna payment fund` and follow the instructions?", e)))?;
        log::debug!("Unlock tx: {:?}", unlock);
        log::info!("Unlock send. tx_hash= {}", unlock.hash().to_string());

        let tx_info = unlock.wait_for_commit().await.map_err(GenericError::new)?;
        log::debug!("tx_info = {:?}", tx_info);
        match tx_info.success {
            Some(true) => log::info!("Wallet successfully unlocked. address = {}", wallet.signer.address),
            Some(false) => return Err(GenericError::new(format!("Failed to unlock wallet. reason={}", tx_info.fail_reason.unwrap_or("Unknown reason".to_string())))),
            None => return Err(GenericError::new(format!("Unknown result from zksync unlock, please check your wallet on zkscan and try again. {:?}", tx_info))),
        }
    }
    Ok(())
}

pub async fn withdraw<S: EthereumSigner + Clone>(
    wallet: Wallet<S>,
    amount: Option<BigDecimal>,
    recipient: Option<String>,
) -> Result<SyncTransactionHandle, GenericError> {
    let balance = wallet
        .get_balance(BlockStatus::Committed, ZKSYNC_TOKEN_NAME)
        .await
        .map_err(GenericError::new)?;
    info!(
        "Wallet funded with {} tGLM available for withdrawal",
        utils::big_uint_to_big_dec(balance.clone())
    );

    info!("Obtaining withdrawal fee");
    let address = wallet.address();
    let withdraw_fee = wallet
        .provider
        .get_tx_fee(TxFeeTypes::Withdraw, address, ZKSYNC_TOKEN_NAME)
        .await
        .map_err(GenericError::new)?
        .total_fee;
    info!(
        "Withdrawal transaction fee {:.5}",
        utils::big_uint_to_big_dec(withdraw_fee.clone())
    );

    let amount = match amount {
        Some(amount) => utils::big_dec_to_big_uint(amount)?,
        None => balance.clone(),
    };
    let withdraw_amount = std::cmp::min(balance - withdraw_fee, amount);
    info!(
        "Withdrawal of {:.5} tGLM started",
        utils::big_uint_to_big_dec(withdraw_amount.clone())
    );

    let recipient_address = match recipient {
        Some(addr) => Address::from_str(&addr[2..]).map_err(GenericError::new)?,
        None => address,
    };

    let withdraw_handle = wallet
        .start_withdraw()
        .token(ZKSYNC_TOKEN_NAME)
        .map_err(GenericError::new)?
        .amount(withdraw_amount)
        .to(recipient_address)
        .send()
        .await
        .map_err(GenericError::new)?;

    debug!("Withdraw handle: {:?}", withdraw_handle);
    Ok(withdraw_handle)
}
