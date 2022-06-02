use std::collections::HashMap;
use std::sync::Arc;

use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use ethabi::Token;
use lazy_static::lazy_static;
use tokio::sync::RwLock;
use uuid::Uuid;
use web3::{
    contract::{Contract, Options},
    error::Error,
    transports::Http,
    types::{Bytes, Transaction, TransactionId, TransactionReceipt, H160, H256, U256, U64},
    Web3,
};

use ya_client_model::NodeId;
use ya_payment_driver::db::models::{Network, TransactionEntity, TransactionStatus, TxType};
use ya_payment_driver::utils::big_dec_to_u256;
use ya_payment_driver::{bus, model::GenericError};

use crate::erc20::transaction::YagnaRawTransaction;
use crate::erc20::{config, eth_utils};

#[derive(Clone, Debug, thiserror::Error)]
pub enum ClientError {
    #[error("{0}")]
    Web3(#[from] Error),
    #[error("{0}")]
    Other(#[from] GenericError),
}

impl ClientError {
    pub fn new(value: impl std::fmt::Display) -> Self {
        Self::Other(GenericError::new(value))
    }
}

impl From<web3::contract::Error> for ClientError {
    fn from(e: web3::contract::Error) -> Self {
        Self::Other(GenericError::new(e))
    }
}

impl From<ClientError> for GenericError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Other(e) => e,
            ClientError::Web3(e) => GenericError::new(e),
        }
    }
}

pub enum PolygonPriority {
    PolygonPrioritySlow,
    PolygonPriorityFast,
    PolygonPriorityExpress,
}

pub enum PolygonGasPriceMethod {
    PolygonGasPriceStatic,
    PolygonGasPriceDynamic,
}

pub const POLYGON_PREFERRED_GAS_PRICES_SLOW: [f64; 6] = [0.0, 10.01, 15.01, 20.01, 25.01, 30.01];
pub const POLYGON_PREFERRED_GAS_PRICES_FAST: [f64; 3] = [0.0, 30.01, 40.01];
pub const POLYGON_PREFERRED_GAS_PRICES_EXPRESS: [f64; 3] = [0.0, 60.01, 100.01];

lazy_static! {
    pub static ref GLM_FAUCET_GAS: U256 = U256::from(90_000);
    pub static ref GLM_TRANSFER_GAS: U256 = U256::from(55_000);
    pub static ref GLM_POLYGON_GAS_LIMIT: U256 = U256::from(100_000);
    static ref WEB3_CLIENT_MAP: Arc<RwLock<HashMap<String, Web3<Http>>>> = Default::default();
}
const CREATE_FAUCET_FUNCTION: &str = "create";
const BALANCE_ERC20_FUNCTION: &str = "balanceOf";
const TRANSFER_ERC20_FUNCTION: &str = "transfer";

pub fn get_polygon_starting_price() -> f64 {
    match get_polygon_priority() {
        PolygonPriority::PolygonPrioritySlow => POLYGON_PREFERRED_GAS_PRICES_SLOW[1],
        PolygonPriority::PolygonPriorityFast => POLYGON_PREFERRED_GAS_PRICES_FAST[1],
        PolygonPriority::PolygonPriorityExpress => POLYGON_PREFERRED_GAS_PRICES_EXPRESS[1],
    }
}

pub fn get_polygon_maximum_price() -> f64 {
    match get_polygon_gas_price_method() {
        PolygonGasPriceMethod::PolygonGasPriceStatic => match get_polygon_priority() {
            PolygonPriority::PolygonPrioritySlow => {
                POLYGON_PREFERRED_GAS_PRICES_SLOW[POLYGON_PREFERRED_GAS_PRICES_SLOW.len() - 1]
            }
            PolygonPriority::PolygonPriorityFast => {
                POLYGON_PREFERRED_GAS_PRICES_FAST[POLYGON_PREFERRED_GAS_PRICES_FAST.len() - 1]
            }
            PolygonPriority::PolygonPriorityExpress => {
                POLYGON_PREFERRED_GAS_PRICES_EXPRESS[POLYGON_PREFERRED_GAS_PRICES_EXPRESS.len() - 1]
            }
        },
        PolygonGasPriceMethod::PolygonGasPriceDynamic => get_polygon_max_gas_price_dynamic(),
    }
}

pub fn get_polygon_max_gas_price_dynamic() -> f64 {
    return std::env::var("POLYGON_MAX_GAS_PRICE_DYNAMIC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000.0f64);
}

pub fn get_polygon_gas_price_method() -> PolygonGasPriceMethod {
    match std::env::var("POLYGON_GAS_PRICE_METHOD")
        .ok()
        .map(|v| v.to_lowercase())
        .as_ref()
        .map(AsRef::as_ref) // Option<&str>
    {
        Some("static") => PolygonGasPriceMethod::PolygonGasPriceStatic,
        Some("dynamic") => PolygonGasPriceMethod::PolygonGasPriceDynamic,
        _ => PolygonGasPriceMethod::PolygonGasPriceDynamic,
    }
}

pub fn get_polygon_priority() -> PolygonPriority {
    match std::env::var("POLYGON_PRIORITY")
        .unwrap_or("default".to_string())
        .to_lowercase()
        .as_str()
    {
        "slow" => PolygonPriority::PolygonPrioritySlow,
        "fast" => PolygonPriority::PolygonPriorityFast,
        "express" => PolygonPriority::PolygonPriorityExpress,
        _ => PolygonPriority::PolygonPrioritySlow,
    }
}

pub async fn get_glm_balance(address: H160, network: Network) -> Result<U256, GenericError> {
    with_clients(network, |client| {
        get_glm_balance_with(client, address, network)
    })
    .await
}

async fn get_glm_balance_with(
    client: Web3<Http>,
    address: H160,
    network: Network,
) -> Result<U256, ClientError> {
    let env = get_env(network);
    let glm_contract = prepare_erc20_contract(&client, &env)?;
    glm_contract
        .query(
            BALANCE_ERC20_FUNCTION,
            (address,),
            None,
            Options::default(),
            None,
        )
        .await
        .map_err(Into::into)
}

pub async fn get_balance(address: H160, network: Network) -> Result<U256, GenericError> {
    with_clients(network, |client| get_balance_with(address, client)).await
}

async fn get_balance_with(address: H160, client: Web3<Http>) -> Result<U256, ClientError> {
    client
        .eth()
        .balance(address, None)
        .await
        .map_err(Into::into)
}

pub async fn get_next_nonce_pending(address: H160, network: Network) -> Result<U256, GenericError> {
    with_clients(network, |client| {
        get_next_nonce_pending_with(client, address)
    })
    .await
}

async fn get_next_nonce_pending_with(
    client: Web3<Http>,
    address: H160,
) -> Result<U256, ClientError> {
    client
        .eth()
        .transaction_count(address, Some(web3::types::BlockNumber::Pending))
        .await
        .map_err(Into::into)
}

pub async fn with_clients<T, F, R>(network: Network, mut f: F) -> Result<T, GenericError>
where
    F: FnMut(Web3<Http>) -> R,
    R: futures::Future<Output = Result<T, ClientError>>,
{
    let clients = get_clients(network).await?;
    let mut last_err: Option<ClientError> = None;

    for client in clients {
        match f(client).await {
            Ok(result) => return Ok(result),
            Err(ClientError::Web3(e)) => match e {
                Error::Internal | Error::Recovery(_) | Error::Rpc(_) | Error::Decoder(_) => {
                    return Err(GenericError::new(e))
                }
                _ => continue,
            },
            Err(e) => last_err.replace(e),
        };
    }

    match last_err {
        Some(e) => Err(e.into()),
        _ => Err(GenericError::new("Web3 clients failed.")),
    }
}

pub async fn block_number(network: Network) -> Result<U64, GenericError> {
    with_clients(network, |client| block_number_with(client)).await
}

async fn block_number_with(client: Web3<Http>) -> Result<U64, ClientError> {
    client.eth().block_number().await.map_err(Into::into)
}

pub async fn sign_faucet_tx(
    address: H160,
    network: Network,
    nonce: U256,
) -> Result<TransactionEntity, GenericError> {
    with_clients(network, |client| {
        sign_faucet_tx_with(client, address, network, nonce)
    })
    .await
}

async fn sign_faucet_tx_with(
    client: Web3<Http>,
    address: H160,
    network: Network,
    nonce: U256,
) -> Result<TransactionEntity, ClientError> {
    let env = get_env(network);
    let contract = prepare_glm_faucet_contract(&client, &env)?;
    let contract = match contract {
        Some(c) => c,
        None => {
            return Err(ClientError::new(
                "Failed to get faucet fn, are you on the right network?",
            ))
        }
    };

    let data = eth_utils::contract_encode(&contract, CREATE_FAUCET_FUNCTION, ()).unwrap();
    let gas_price = client.eth().gas_price().await.map_err(GenericError::new)?;
    let tx = YagnaRawTransaction {
        nonce,
        to: Some(contract.address()),
        value: U256::from(0),
        gas_price,
        gas: *GLM_FAUCET_GAS,
        data,
    };
    //let chain_id = network as u64;
    //let node_id = NodeId::from(address.as_ref());
    //let signature = bus::sign(node_id, eth_utils::get_tx_hash(&tx, chain_id)).await?;

    Ok(create_dao_entity(
        nonce,
        address,
        gas_price.to_string(),
        Some(gas_price.to_string()),
        GLM_FAUCET_GAS.as_u32() as i32,
        serde_json::to_string(&tx).map_err(GenericError::new)?,
        network,
        Utc::now(),
        TxType::Faucet,
        None,
    ))
}

pub async fn sign_raw_transfer_transaction(
    address: H160,
    network: Network,
    tx: &YagnaRawTransaction,
) -> Result<Vec<u8>, GenericError> {
    let chain_id = network as u64;
    let node_id = NodeId::from(address.as_ref());
    let signature = bus::sign(node_id, eth_utils::get_tx_hash(&tx, chain_id)).await?;
    Ok(signature)
}

pub async fn prepare_raw_transaction(
    _address: H160,
    recipient: H160,
    amount: U256,
    network: Network,
    nonce: U256,
    gas_price_override: Option<U256>,
    gas_limit_override: Option<u32>,
) -> Result<YagnaRawTransaction, GenericError> {
    with_clients(network, |client| {
        prepare_raw_transaction_with(
            client,
            _address,
            recipient,
            amount,
            network,
            nonce,
            gas_price_override,
            gas_limit_override,
        )
    })
    .await
}

async fn prepare_raw_transaction_with(
    client: Web3<Http>,
    _address: H160,
    recipient: H160,
    amount: U256,
    network: Network,
    nonce: U256,
    gas_price_override: Option<U256>,
    gas_limit_override: Option<u32>,
) -> Result<YagnaRawTransaction, ClientError> {
    let env = get_env(network);
    let contract = prepare_erc20_contract(&client, &env)?;
    let data = eth_utils::contract_encode(&contract, TRANSFER_ERC20_FUNCTION, (recipient, amount))
        .map_err(GenericError::new)?;

    //get gas price from network in not provided
    let gas_price = match gas_price_override {
        Some(gas_price_new) => gas_price_new,
        None => {
            let small_gas_bump = U256::from(1000);
            let mut gas_price_from_network =
                client.eth().gas_price().await.map_err(GenericError::new)?;

            //add small amount of gas to be first in queue
            if gas_price_from_network / 1000 > small_gas_bump {
                gas_price_from_network += small_gas_bump;
            }
            gas_price_from_network
        }
    };

    let gas_limit = match network {
        Network::Polygon => gas_limit_override.map_or(*GLM_POLYGON_GAS_LIMIT, |v| U256::from(v)),
        Network::Mumbai => gas_limit_override.map_or(*GLM_POLYGON_GAS_LIMIT, |v| U256::from(v)),
        _ => gas_limit_override.map_or(*GLM_TRANSFER_GAS, |v| U256::from(v)),
    };

    let tx = YagnaRawTransaction {
        nonce,
        to: Some(contract.address()),
        value: U256::from(0),
        gas_price,
        gas: gas_limit,
        data,
    };

    Ok(tx)
}

pub async fn send_tx(signed_tx: Vec<u8>, network: Network) -> Result<H256, GenericError> {
    with_clients(network, |client| send_tx_with(client, signed_tx.clone())).await
}

async fn send_tx_with(client: Web3<Http>, signed_tx: Vec<u8>) -> Result<H256, ClientError> {
    client
        .eth()
        .send_raw_transaction(Bytes::from(signed_tx))
        .await
        .map_err(Into::into)
}

pub struct TransactionChainStatus {
    pub exists_on_chain: bool,
    pub pending: bool,
    pub confirmed: bool,
    pub succeeded: bool,
    pub gas_used: Option<U256>,
    pub gas_price: Option<U256>,
}

pub async fn get_tx_on_chain_status(
    tx_hash: H256,
    current_block: Option<u64>,
    network: Network,
) -> Result<TransactionChainStatus, GenericError> {
    let mut res = TransactionChainStatus {
        exists_on_chain: false,
        pending: false,
        confirmed: false,
        succeeded: false,
        gas_price: None,
        gas_used: None,
    };
    let env = get_env(network);
    let tx = get_tx_receipt(tx_hash, network).await?;
    if let Some(tx) = tx {
        res.exists_on_chain = true;
        res.gas_used = tx.gas_used;
        const TRANSACTION_STATUS_SUCCESS: u64 = 1;
        if tx.status == Some(ethereum_types::U64::from(TRANSACTION_STATUS_SUCCESS)) {
            res.succeeded = true;
        }
        if let Some(tx_bn) = tx.block_number {
            // TODO: Store tx.block_number in DB and check only once after required_confirmations.
            log::trace!(
                "is_tx_confirmed? tb + rq - 1 <= cb. tb={}, rq={}, cb={}",
                tx_bn,
                env.required_confirmations,
                current_block.unwrap_or(0)
            );
            // tx.block_number is the first confirmation, so we need to - 1
            if let Some(current_block) = current_block {
                if tx_bn.as_u64() + env.required_confirmations - 1 <= current_block {
                    res.confirmed = true;
                }
            }
            let transaction = get_tx_from_network(tx_hash, network).await?;
            if let Some(t) = transaction {
                res.gas_price = Some(t.gas_price);
            }
        } else {
        }
    } else {
        let transaction = get_tx_from_network(tx_hash, network).await?;
        if let Some(_transaction) = transaction {
            res.exists_on_chain = true;
            res.pending = true;
        }
    }
    Ok(res)
}

//unused but tested that it is working for transfers
pub async fn decode_encoded_transaction_data(
    network: Network,
    encoded: &str,
) -> Result<(ethereum_types::Address, ethereum_types::U256), GenericError> {
    with_clients(network, |client| {
        decode_encoded_transaction_data_with(client, network, encoded)
    })
    .await
}

async fn decode_encoded_transaction_data_with(
    client: Web3<Http>,
    network: Network,
    encoded: &str,
) -> Result<(ethereum_types::Address, ethereum_types::U256), ClientError> {
    let env = get_env(network);
    let contract = prepare_erc20_contract(&client, &env)?;
    let raw_tx: YagnaRawTransaction = serde_json::from_str(encoded).map_err(GenericError::new)?;

    let tokens = eth_utils::contract_decode(&contract, TRANSFER_ERC20_FUNCTION, raw_tx.data)
        .map_err(GenericError::new)?;
    let mut address: Option<H160> = None;
    let mut amount: Option<U256> = None;
    for token in tokens {
        match token {
            Token::Address(val) => address = Some(val),
            Token::Uint(am) => amount = Some(am),
            _ => {}
        };
    }
    if let Some(add) = address {
        if let Some(am) = amount {
            return Ok((add, am));
        }
    }
    Err(GenericError::new("Failed to parse tokens").into())
}

pub async fn get_tx_from_network(
    tx_hash: H256,
    network: Network,
) -> Result<Option<Transaction>, GenericError> {
    with_clients(network, |client| get_tx_from_network_with(client, tx_hash)).await
}

async fn get_tx_from_network_with(
    client: Web3<Http>,
    tx_hash: H256,
) -> Result<Option<Transaction>, ClientError> {
    client
        .eth()
        .transaction(TransactionId::from(tx_hash))
        .await
        .map_err(Into::into)
}

pub async fn get_tx_receipt(
    tx_hash: H256,
    network: Network,
) -> Result<Option<TransactionReceipt>, GenericError> {
    with_clients(network, |client| get_tx_receipt_with(client, tx_hash)).await
}

async fn get_tx_receipt_with(
    client: Web3<Http>,
    tx_hash: H256,
) -> Result<Option<TransactionReceipt>, ClientError> {
    client
        .eth()
        .transaction_receipt(tx_hash)
        .await
        .map_err(Into::into)
}

fn get_rpc_addr_from_env(network: Network) -> Vec<String> {
    match network {
        Network::Mainnet => {
            collect_rpc_addr_from("MAINNET_GETH_ADDR", "https://geth.golem.network:55555")
        }
        Network::Rinkeby => collect_rpc_addr_from(
            "RINKEBY_GETH_ADDR",
            "http://geth.testnet.golem.network:55555",
        ),
        Network::Goerli => {
            collect_rpc_addr_from("GOERLI_GETH_ADDR", "https://rpc.goerli.mudit.blog")
        }
        Network::Polygon => collect_rpc_addr_from(
            "POLYGON_GETH_ADDR",
            "https://bor.golem.network,https://polygon-rpc.com",
        ),
        Network::Mumbai => collect_rpc_addr_from(
            "MUMBAI_GETH_ADDR",
            "https://matic-mumbai.chainstacklabs.com",
        ),
    }
}

fn collect_rpc_addr_from(env: &str, default: &str) -> Vec<String> {
    std::env::var(env)
        .ok()
        .unwrap_or(default.to_string())
        .split(',')
        .map(|path| path.to_string())
        .collect()
}

async fn get_clients(network: Network) -> Result<Vec<Web3<Http>>, GenericError> {
    let geth_addrs = get_rpc_addr_from_env(network);
    let mut clients: Vec<Web3<Http>> = Default::default();

    for geth_addr in geth_addrs {
        {
            let client_map = WEB3_CLIENT_MAP.read().await;
            if let Some(client) = client_map.get(&geth_addr).cloned() {
                clients.push(client);
                continue;
            }
        }

        let transport = match web3::transports::Http::new(&geth_addr) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let client = Web3::new(transport);

        let mut client_map = WEB3_CLIENT_MAP.write().await;
        client_map.insert(geth_addr, client.clone());

        clients.push(client);
    }

    Ok(clients)
}

fn get_env(network: Network) -> config::EnvConfiguration {
    match network {
        Network::Mainnet => *config::MAINNET_CONFIG,
        Network::Rinkeby => *config::RINKEBY_CONFIG,
        Network::Goerli => *config::GOERLI_CONFIG,
        Network::Mumbai => *config::MUMBAI_CONFIG,
        Network::Polygon => *config::POLYGON_MAINNET_CONFIG,
    }
}

fn prepare_contract(
    ethereum_client: &Web3<Http>,
    address: H160,
    json_abi: &[u8],
) -> Result<Contract<Http>, GenericError> {
    let contract =
        Contract::from_json(ethereum_client.eth(), address, json_abi).map_err(GenericError::new)?;

    Ok(contract)
}

fn prepare_erc20_contract(
    ethereum_client: &Web3<Http>,
    env: &config::EnvConfiguration,
) -> Result<Contract<Http>, GenericError> {
    prepare_contract(
        ethereum_client,
        env.glm_contract_address,
        include_bytes!("../contracts/ierc20.json"),
    )
}

fn prepare_glm_faucet_contract(
    ethereum_client: &Web3<Http>,
    env: &config::EnvConfiguration,
) -> Result<Option<Contract<Http>>, GenericError> {
    if let Some(glm_faucet_address) = env.glm_faucet_address {
        Ok(Some(prepare_contract(
            ethereum_client,
            glm_faucet_address,
            include_bytes!("../contracts/faucet.json"),
        )?))
    } else {
        Ok(None)
    }
}

pub fn create_dao_entity(
    nonce: U256,
    sender: H160,
    starting_gas_price: String,
    max_gas_price: Option<String>,
    gas_limit: i32,
    encoded_raw_tx: String,
    network: Network,
    timestamp: DateTime<Utc>,
    tx_type: TxType,
    amount: Option<BigDecimal>,
) -> TransactionEntity {
    let current_naive_time = timestamp.naive_utc();
    TransactionEntity {
        tx_id: Uuid::new_v4().to_string(),
        sender: format!("0x{:x}", sender),
        nonce: nonce.as_u32() as i32,
        time_created: current_naive_time,
        time_last_action: current_naive_time,
        time_sent: None,
        time_confirmed: None,
        max_gas_price,
        final_gas_used: None,
        amount_base: Some("0".to_string()),
        amount_erc20: amount.as_ref().map(|a| big_dec_to_u256(a).to_string()),
        gas_limit: Some(gas_limit),
        starting_gas_price: Some(starting_gas_price),
        current_gas_price: None,
        encoded: encoded_raw_tx,
        status: TransactionStatus::Created as i32,
        tx_type: tx_type as i32,
        signature: None,
        tmp_onchain_txs: None,
        final_tx: None,
        network,
        last_error_msg: None,
        resent_times: 0,
    }
}

pub fn get_max_gas_costs(db_tx: &TransactionEntity) -> Result<U256, GenericError> {
    let raw_tx: YagnaRawTransaction =
        serde_json::from_str(&db_tx.encoded).map_err(GenericError::new)?;
    Ok(raw_tx.gas_price * raw_tx.gas)
}

pub fn get_gas_price_from_db_tx(db_tx: &TransactionEntity) -> Result<U256, GenericError> {
    let raw_tx: YagnaRawTransaction =
        serde_json::from_str(&db_tx.encoded).map_err(GenericError::new)?;
    Ok(raw_tx.gas_price)
}
