/*
    Wallet functions on zksync.
*/

// External crates
use bigdecimal::{BigDecimal, Zero};
use num_bigint::BigUint;
use std::env;
use std::str::FromStr;
use tokio_compat_02::FutureExt;
use zksync::operations::SyncTransactionHandle;
use zksync::types::BlockStatus;
use zksync::zksync_types::{
    tokens::ChangePubKeyFeeTypeArg, tx::TxHash, Address, Nonce, TxFeeTypes, H256,
};
use zksync::{
    provider::{Provider, RpcProvider},
    Network as ZkNetwork, Wallet, WalletCredentials,
};
use zksync_eth_signer::EthereumSigner;

// Workspace uses
use ya_payment_driver::{
    db::models::Network,
    model::{AccountMode, Enter, Exit, GenericError, Init, PaymentDetails},
    utils as base_utils,
};

// Local uses
use crate::{
    network::get_network_token,
    zksync::{faucet, signer::YagnaEthSigner, utils},
    DEFAULT_NETWORK,
};
use zksync::zksync_types::tx::ChangePubKeyType;

pub async fn account_balance(address: &str, network: Network) -> Result<BigDecimal, GenericError> {
    let pub_address = Address::from_str(&address[2..]).map_err(GenericError::new)?;
    let acc_info = get_provider(network)
        .account_info(pub_address)
        .compat()
        .await
        .map_err(GenericError::new)?;
    // TODO: implement tokens, replace None
    let token = get_network_token(network, None)?;
    let balance_com = acc_info
        .committed
        .balances
        .get(&token)
        .map(|x| x.0.clone())
        .unwrap_or_else(BigUint::zero);
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
    let network = msg.network().unwrap_or_default();
    let network = Network::from_str(&network).map_err(GenericError::new)?;

    if mode.contains(AccountMode::SEND) {
        let wallet = get_wallet(&address, network).await?;
        unlock_wallet(&wallet, network).await.map_err(|e| {
            GenericError::new(format!(
                "{:?}  HINT: Did you run `yagna payment fund` and follow the instructions?",
                e
            ))
        })?;
    }
    Ok(())
}

pub async fn fund(address: &str, network: Network) -> Result<(), GenericError> {
    if network == Network::Mainnet {
        return Err(GenericError::new("Wallet can not be funded on mainnet."));
    }
    faucet::request_tglm(address, network).await?;
    Ok(())
}

pub async fn exit(msg: &Exit) -> Result<String, GenericError> {
    let network = msg.network().unwrap_or_else(|| DEFAULT_NETWORK.to_string());
    let network = Network::from_str(&network).map_err(GenericError::new)?;
    let wallet = get_wallet(&msg.sender(), network).await?;

    let token = get_network_token(network, None)?;
    let balance = get_balance(&wallet, &token).await?;
    let unlock_fee = get_unlock_fee(&wallet, &token).await?;
    let withdraw_fee = get_withdraw_fee(&wallet, &token).await?;
    let total_fee = unlock_fee + withdraw_fee;
    if balance < total_fee {
        return Err(GenericError::new(format!(
            "Not enough balance to exit. Minimum required balance to pay withdraw fees is {} {}",
            utils::big_uint_to_big_dec(total_fee),
            token
        )));
    }

    unlock_wallet(&wallet, network).await?;
    let tx_handle = withdraw(wallet, network, msg.amount(), msg.to()).await?;
    let tx_info = tx_handle
        .wait_for_commit()
        .compat()
        .await
        .map_err(GenericError::new)?;

    match tx_info.success {
        Some(true) => Ok(hash_to_hex(tx_handle.hash())),
        Some(false) => Err(GenericError::new(
            tx_info
                .fail_reason
                .unwrap_or_else(|| "Unknown failure reason".to_string()),
        )),
        None => Err(GenericError::new("Transaction time-outed")),
    }
}

pub async fn enter(msg: Enter) -> Result<String, GenericError> {
    let network = msg.network.unwrap_or_else(|| DEFAULT_NETWORK.to_string());
    let network = Network::from_str(&network).map_err(GenericError::new)?;
    let wallet = get_wallet(&msg.address, network).await?;

    let tx_hash = deposit(wallet, network, msg.amount).await?;

    Ok(hex::encode(tx_hash.as_fixed_bytes()))
}

pub async fn get_tx_fee(address: &str, network: Network) -> Result<BigDecimal, GenericError> {
    let token = get_network_token(network, None)?;
    let wallet = get_wallet(address, network).await?;
    let tx_fee = wallet
        .provider
        .get_tx_fee(TxFeeTypes::Transfer, wallet.address(), token.as_str())
        .await
        .map_err(GenericError::new)?
        .total_fee;
    let tx_fee_bigdec = utils::big_uint_to_big_dec(tx_fee);

    log::debug!("Transaction fee {:.5} {}", tx_fee_bigdec, token.as_str());
    Ok(tx_fee_bigdec)
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
    let account_info = match provider.account_info(addr).compat().await {
        Ok(i) => i,
        Err(e) => {
            log::error!("Unable to get account info, failed to get nonce. {:?}", e);
            return 0;
        }
    };
    *account_info.committed.nonce
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
    let token = get_network_token(network, None)?;

    let balance = get_balance(&wallet, &token).await?;
    log::debug!("balance before transfer={}", balance);

    let transfer_builder = wallet
        .start_transfer()
        .nonce(Nonce(nonce))
        .str_to(&details.recipient[2..])
        .map_err(GenericError::new)?
        .token(token.as_str())
        .map_err(GenericError::new)?
        .amount(amount.clone());
    log::debug!(
        "transfer raw data. nonce={}, to={}, token={}, amount={}",
        nonce,
        &details.recipient,
        token,
        amount
    );
    let transfer = transfer_builder
        .send()
        .compat()
        .await
        .map_err(GenericError::new)?;

    let tx_hash = hex::encode(transfer.hash());
    log::info!("Created zksync transaction with hash={}", tx_hash);
    Ok(tx_hash)
}

pub async fn check_tx(tx_hash: &str, network: Network) -> Option<Result<(), String>> {
    let provider = get_provider(network);
    let tx_hash = format!("sync-tx:{}", tx_hash);
    let tx_hash = TxHash::from_str(&tx_hash).unwrap();
    let tx_info = provider.tx_info(tx_hash).compat().await.unwrap();
    log::trace!("tx_info: {:?}", tx_info);
    match tx_info.success {
        None => None,
        Some(true) => Some(Ok(())),
        Some(false) => match tx_info.fail_reason {
            Some(err) => Some(Err(err)),
            None => Some(Err("Unknown failure".to_string())),
        },
    }
}

#[derive(serde::Deserialize)]
struct TxRespObj {
    to: String,
    from: String,
    amount: String,
    created_at: String,
}

pub async fn verify_tx(tx_hash: &str, network: Network) -> Result<PaymentDetails, GenericError> {
    let provider_url = get_rpc_addr(network);

    // HACK: Get the transaction data from v0.1 api
    let api_url = provider_url.replace("/jsrpc", "/api/v0.1");
    let req_url = format!("{}/transactions_all/{}", api_url, tx_hash);
    log::debug!("Request URL: {}", &req_url);

    let client = awc::Client::new();
    let response = client
        .get(req_url)
        .send()
        .await
        .map_err(GenericError::new)?
        .body()
        .await
        .map_err(GenericError::new)?;
    let response = String::from_utf8_lossy(response.as_ref());
    log::trace!("Request response: {}", &response);
    let v: TxRespObj = serde_json::from_str(&response).map_err(GenericError::new)?;

    let recipient = v.to;
    let sender = v.from;
    let amount =
        utils::big_uint_to_big_dec(BigUint::from_str(&v.amount).map_err(GenericError::new)?);
    let date_str = format!("{}Z", v.created_at);
    let date = Some(chrono::DateTime::from_str(&date_str).map_err(GenericError::new)?);
    let details = PaymentDetails {
        recipient,
        sender,
        amount,
        date,
    };
    log::debug!("PaymentDetails from server: {:?}", &details);

    Ok(details)
}

fn get_provider(network: Network) -> RpcProvider {
    RpcProvider::from_addr_and_network(get_rpc_addr(network), get_zk_network(network))
}

fn get_rpc_addr(network: Network) -> String {
    match network {
        Network::Mainnet => env::var("ZKSYNC_MAINNET_RPC_ADDRESS")
            .unwrap_or_else(|_| "https://api.zksync.golem.network/jsrpc".to_string()),
        Network::Rinkeby => env::var("ZKSYNC_RINKEBY_RPC_ADDRESS")
            .unwrap_or_else(|_| "https://rinkeby-api.zksync.golem.network/jsrpc".to_string()),
        Network::Goerli => panic!("Goerli not supported on zksync"),
        Network::Polygon => panic!("Polygon not supported on zksync"),
        Network::Mumbai => panic!("Mumbai not supported on zksync"),
    }
}

fn get_ethereum_node_addr_from_env(network: Network) -> String {
    match network {
        Network::Mainnet => env::var("MAINNET_GETH_ADDR")
            .unwrap_or_else(|_| "https://geth.golem.network:55555".to_string()),
        Network::Rinkeby => env::var("RINKEBY_GETH_ADDR")
            .unwrap_or_else(|_| "http://geth.testnet.golem.network:55555".to_string()),
        Network::Goerli => panic!("Goerli not supported on zksync"),
        Network::Polygon => panic!("Polygon mainnet not supported on zksync"),
        Network::Mumbai => panic!("Polygon mumbai not supported on zksync"),
    }
}

fn get_ethereum_confirmation_timeout() -> std::time::Duration {
    let value = std::env::var("ZKSYNC_ETH_CONFIRMATION_TIMEOUT_SECONDS")
        .unwrap_or_else(|_| "60".to_owned());
    std::time::Duration::from_secs(value.parse::<u64>().unwrap())
}

async fn get_wallet(
    address: &str,
    network: Network,
) -> Result<Wallet<YagnaEthSigner, RpcProvider>, GenericError> {
    log::debug!("get_wallet {:?}", address);
    let addr = Address::from_str(&address[2..]).map_err(GenericError::new)?;
    let provider = get_provider(network);
    let signer = YagnaEthSigner::new(addr);
    let credentials = WalletCredentials::from_eth_signer(addr, signer, get_zk_network(network))
        .compat()
        .await
        .map_err(GenericError::new)?;
    let wallet = Wallet::new(provider, credentials)
        .compat()
        .await
        .map_err(GenericError::new)?;
    Ok(wallet)
}

fn get_zk_network(network: Network) -> ZkNetwork {
    ZkNetwork::from_str(&network.to_string()).unwrap() // _or(ZkNetwork::Rinkeby)
}

async fn unlock_wallet<S: EthereumSigner + Clone, P: Provider + Clone>(
    wallet: &Wallet<S, P>,
    network: Network,
) -> Result<(), GenericError> {
    log::debug!("unlock_wallet");
    if !wallet
        .is_signing_key_set()
        .compat()
        .await
        .map_err(GenericError::new)?
    {
        log::info!("Unlocking wallet... address = {}", wallet.signer.address);
        let token = get_network_token(network, None)?;
        let balance = get_balance(wallet, &token).await?;
        let unlock_fee = get_unlock_fee(wallet, &token).await?;
        if unlock_fee > balance {
            return Err(GenericError::new("Not enough balance to unlock account"));
        }

        let unlock = wallet
            .start_change_pubkey()
            .fee(unlock_fee)
            .fee_token(token.as_str())
            .map_err(|e| {
                GenericError::new(format!("Failed to create change_pubkey request: {}", e))
            })?
            .send()
            .compat()
            .await
            .map_err(|e| {
                GenericError::new(format!("Failed to send change_pubkey request: {}", e))
            })?;
        log::info!("Unlock send. tx_hash= {}", unlock.hash().to_string());

        let tx_info = unlock
            .wait_for_commit()
            .compat()
            .await
            .map_err(GenericError::new)?;
        log::debug!("tx_info = {:?}", tx_info);
        match tx_info.success {
            Some(true) => log::info!("Wallet successfully unlocked. address = {}", wallet.signer.address),
            Some(false) => return Err(GenericError::new(format!("Failed to unlock wallet. reason={}", tx_info.fail_reason.unwrap_or_else(|| "Unknown reason".to_string())))),
            None => return Err(GenericError::new(format!("Unknown result from zksync unlock, please check your wallet on zkscan and try again. {:?}", tx_info))),
        }
    }
    Ok(())
}

pub async fn withdraw<S: EthereumSigner + Clone, P: Provider + Clone>(
    wallet: Wallet<S, P>,
    network: Network,
    amount: Option<BigDecimal>,
    recipient: Option<String>,
) -> Result<SyncTransactionHandle<P>, GenericError> {
    let token = get_network_token(network, None)?;
    let balance = get_balance(&wallet, &token).await?;
    info!(
        "Wallet funded with {} {} available for withdrawal",
        utils::big_uint_to_big_dec(balance.clone()),
        token
    );

    info!("Obtaining withdrawal fee");
    let withdraw_fee = get_withdraw_fee(&wallet, &token).await?;
    info!(
        "Withdrawal transaction fee {:.5} {}",
        utils::big_uint_to_big_dec(withdraw_fee.clone()),
        token
    );
    if withdraw_fee > balance {
        return Err(GenericError::new("Not enough balance to withdraw"));
    }

    let amount = match amount {
        Some(amount) => utils::big_dec_to_big_uint(amount)?,
        None => balance.clone(),
    };
    let withdraw_amount = std::cmp::min(balance - &withdraw_fee, amount);
    info!(
        "Withdrawal of {:.5} {} started",
        utils::big_uint_to_big_dec(withdraw_amount.clone()),
        token
    );

    let recipient_address = match recipient {
        Some(addr) => Address::from_str(&addr[2..]).map_err(GenericError::new)?,
        None => wallet.address(),
    };

    let withdraw_builder = wallet
        .start_withdraw()
        .fee(withdraw_fee)
        .token(token.as_str())
        .map_err(GenericError::new)?
        .amount(withdraw_amount.clone())
        .to(recipient_address);
    log::debug!(
        "Withdrawal raw data. token={}, amount={}, to={}",
        token,
        withdraw_amount,
        recipient_address
    );
    let withdraw_handle = withdraw_builder
        .send()
        .compat()
        .await
        .map_err(GenericError::new)?;

    Ok(withdraw_handle)
}

async fn get_balance<S: EthereumSigner + Clone, P: Provider + Clone>(
    wallet: &Wallet<S, P>,
    token: &str,
) -> Result<BigUint, GenericError> {
    let balance = wallet
        .get_balance(BlockStatus::Committed, token)
        .compat()
        .await
        .map_err(GenericError::new)?;
    Ok(balance)
}

async fn get_withdraw_fee<S: EthereumSigner + Clone, P: Provider + Clone>(
    wallet: &Wallet<S, P>,
    token: &str,
) -> Result<BigUint, GenericError> {
    let withdraw_fee = wallet
        .provider
        .get_tx_fee(TxFeeTypes::Withdraw, wallet.address(), token)
        .compat()
        .await
        .map_err(GenericError::new)?
        .total_fee;
    Ok(withdraw_fee)
}

async fn get_unlock_fee<S: EthereumSigner + Clone, P: Provider + Clone>(
    wallet: &Wallet<S, P>,
    token: &str,
) -> Result<BigUint, GenericError> {
    if wallet
        .is_signing_key_set()
        .compat()
        .await
        .map_err(GenericError::new)?
    {
        return Ok(BigUint::zero());
    }
    let unlock_fee = wallet
        .provider
        .get_tx_fee(
            TxFeeTypes::ChangePubKey(ChangePubKeyFeeTypeArg::ContractsV4Version(
                ChangePubKeyType::ECDSA,
            )),
            wallet.address(),
            token,
        )
        .compat()
        .await
        .map_err(GenericError::new)?
        .total_fee;
    Ok(unlock_fee)
}

pub async fn deposit<S: EthereumSigner + Clone, P: Provider + Clone>(
    wallet: Wallet<S, P>,
    network: Network,
    amount: BigDecimal,
) -> Result<H256, GenericError> {
    let token = get_network_token(network, None)?;
    let amount = base_utils::big_dec_to_u256(&amount);
    let address = wallet.address();

    log::info!(
        "Starting deposit into ZkSync network. Address {:#x}, amount: {} of {}",
        address,
        amount,
        token
    );

    let mut ethereum = wallet
        .ethereum(get_ethereum_node_addr_from_env(network))
        .await
        .map_err(GenericError::new)?;
    ethereum.set_confirmation_timeout(get_ethereum_confirmation_timeout());

    if !ethereum
        .is_limited_erc20_deposit_approved(token.as_str(), amount)
        .await
        .unwrap()
    {
        let tx = ethereum
            .limited_approve_erc20_token_deposits(token.as_str(), amount)
            .await
            .map_err(GenericError::new)?;
        info!(
            "Approve erc20 token for ZkSync deposit. Tx: https://rinkeby.etherscan.io/tx/{:#x}",
            tx
        );

        ethereum.wait_for_tx(tx).await.map_err(GenericError::new)?;
    }

    let deposit_tx_hash = ethereum
        .deposit(token.as_str(), amount, address)
        .await
        .map_err(GenericError::new)?;
    info!(
        "Check out deposit transaction at https://rinkeby.etherscan.io/tx/{:#x}",
        deposit_tx_hash
    );

    Ok(deposit_tx_hash)
}
