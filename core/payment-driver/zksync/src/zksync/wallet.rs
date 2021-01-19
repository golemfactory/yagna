/*
    Wallet functions on zksync.
*/

// External crates
use bigdecimal::{BigDecimal, Zero};
use futures3::TryFutureExt;
use lazy_static::lazy_static;
use num::BigUint;
use std::env;
use std::str::FromStr;
use zksync::operations::SyncTransactionHandle;
use zksync::types::BlockStatus;
use zksync::zksync_types::{tx::TxHash, Address, TxFeeTypes};
use zksync::{Network, Provider, Wallet, WalletCredentials};
use zksync_eth_signer::EthereumSigner;

// Workspace uses
use ya_payment_driver::model::{AccountMode, Exit, GenericError, Init, PaymentDetails};

// Local uses
use crate::{
    zksync::{faucet, signer::YagnaEthSigner, utils},
    ZKSYNC_TOKEN_NAME,
};

lazy_static! {
    pub static ref NETWORK: Network = {
        let chain_id = env::var("ZKSYNC_CHAIN")
            .unwrap_or("rinkeby".to_string())
            .to_lowercase();
        match chain_id.parse() {
            Ok(network) => network,
            Err(_) => panic!(format!("Invalid chain id: {}", chain_id)),
        }
    };
    static ref PROVIDER: Provider = match env::var("ZKSYNC_RPC_ADDRESS").ok() {
        Some(rpc_addr) => Provider::from_addr(rpc_addr),
        None => Provider::new(*NETWORK),
    };
}

pub async fn account_balance(address: &str) -> Result<BigDecimal, GenericError> {
    let pub_address = Address::from_str(&address[2..]).map_err(GenericError::new)?;
    let acc_info = get_provider()
        .account_info(pub_address)
        .await
        .map_err(GenericError::new)?;
    let balance_com = acc_info
        .committed
        .balances
        .get(ZKSYNC_TOKEN_NAME)
        .map(|x| x.0.clone())
        .unwrap_or(BigUint::zero());
    let balance = utils::big_uint_to_big_dec(balance_com);
    log::debug!("account_balance. address={}, balance={}", address, &balance);
    Ok(balance)
}

pub async fn init_wallet(msg: &Init) -> Result<(), GenericError> {
    log::debug!("init_wallet. network={}, msg={:?}", *NETWORK, msg);
    let mode = msg.mode();
    let address = msg.address().clone();

    if mode.contains(AccountMode::SEND) {
        if *NETWORK != Network::Mainnet {
            faucet::request_ngnt(&address).await?;
        }
        get_wallet(&address).and_then(unlock_wallet).await?;
    }
    Ok(())
}

pub async fn exit(address: String, msg: &Exit) -> Result<(), GenericError> {
    let wallet = get_wallet(&address).await?;
    let tx_handle = withdraw(wallet, msg.amount()).await?;
    let tx_info = tx_handle
        .wait_for_commit()
        .await
        .map_err(GenericError::new)?;
    match tx_info.success {
        Some(true) => Ok(()),
        Some(false) => Err(GenericError::new(tx_info.fail_reason.unwrap())),
        None => Err(GenericError::new("timeout?")),
    }
}

pub async fn get_nonce(address: &str) -> u32 {
    let addr = match Address::from_str(&address[2..]) {
        Ok(a) => a,
        Err(e) => {
            log::error!("Unable to parse address, failed to get nonce. {:?}", e);
            return 0;
        }
    };
    let provider = get_provider();
    let account_info = match provider.account_info(addr).await {
        Ok(i) => i,
        Err(e) => {
            log::error!("Unable to get account info, failed to get nonce. {:?}", e);
            return 0;
        }
    };
    account_info.committed.nonce
}

pub async fn make_transfer(details: &PaymentDetails, nonce: u32) -> Result<String, GenericError> {
    log::debug!("make_transfer. {:?}", details);
    let amount = details.amount.clone();
    let amount = utils::big_dec_to_big_uint(amount)?;
    let amount = utils::pack_up(&amount);

    let sender = details.sender.clone();

    let wallet = get_wallet(&sender).await?;

    let balance = wallet
        .get_balance(BlockStatus::Committed, "GNT")
        .await
        .map_err(GenericError::new)?;
    log::debug!("balance before transfer={}", balance);

    let transfer = wallet
        .start_transfer()
        .nonce(nonce)
        .str_to(&details.recipient[2..])
        .map_err(GenericError::new)?
        .token(ZKSYNC_TOKEN_NAME)
        .map_err(GenericError::new)?
        .amount(amount)
        .send()
        .await
        .map_err(GenericError::new)?;

    let tx_hash = hex::encode(transfer.hash());
    log::info!("Created zksync transaction with hash={}", tx_hash);
    Ok(tx_hash)
}

pub async fn check_tx(tx_hash: &str) -> Option<bool> {
    let provider = get_provider();
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

fn get_provider() -> Provider {
    (*PROVIDER).clone()
}

async fn get_wallet(address: &str) -> Result<Wallet<YagnaEthSigner>, GenericError> {
    log::debug!("get_wallet {:?}", address);
    let addr = Address::from_str(&address[2..]).map_err(GenericError::new)?;
    let provider = get_provider();
    let signer = YagnaEthSigner::new(addr);
    let credentials = WalletCredentials::from_eth_signer(addr, signer, *NETWORK)
        .await
        .map_err(GenericError::new)?;
    let wallet = Wallet::new(provider, credentials)
        .await
        .map_err(GenericError::new)?;
    Ok(wallet)
}

async fn unlock_wallet<S: EthereumSigner + Clone>(wallet: Wallet<S>) -> Result<(), GenericError> {
    log::debug!("unlock_wallet");
    if !wallet
        .is_signing_key_set()
        .await
        .map_err(GenericError::new)?
    {
        log::info!("Unlocking wallet... address = {}", wallet.signer.address);
        let unlock = wallet
            .start_change_pubkey()
            .fee_token(ZKSYNC_TOKEN_NAME)
            .map_err(GenericError::new)?
            .send()
            .await
            .map_err(GenericError::new)?;
        info!("Unlock tx: {:?}", unlock);
        let tx_info = unlock.wait_for_commit().await.map_err(GenericError::new)?;
        log::info!("Wallet unlocked. tx_info = {:?}", tx_info);
    }
    Ok(())
}

pub async fn withdraw<S: EthereumSigner + Clone>(
    wallet: Wallet<S>,
    amount: Option<BigDecimal>,
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

    let withdraw_handle = wallet
        .start_withdraw()
        .token(ZKSYNC_TOKEN_NAME)
        .map_err(GenericError::new)?
        .amount(withdraw_amount)
        .to(address)
        .send()
        .await
        .map_err(GenericError::new)?;

    debug!("Withdraw handle: {:?}", withdraw_handle);
    Ok(withdraw_handle)
}
