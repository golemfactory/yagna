use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use num_traits::FromPrimitive;
use sha3::{Digest, Sha3_512};
use std::env;
use web3::contract::{Contract, Options};
use web3::transports::Http;
use web3::types::{Bytes, Transaction, TransactionId, TransactionReceipt, H160, H256, U256, U64};
use web3::Web3;

use ya_client_model::NodeId;
use ya_payment_driver::db::models::{Network, TransactionEntity, TransactionStatus, TxType};
use ya_payment_driver::{bus, model::GenericError, utils as base_utils};

use crate::erc20::transaction::YagnaRawTransaction;
use crate::erc20::{config, eth_utils};
use ethabi::Token;

lazy_static! {
    pub static ref GLM_FAUCET_GAS: U256 = U256::from(90_000);
    pub static ref GLM_TRANSFER_GAS: U256 = U256::from(55_000);
    pub static ref GLM_POLYGON_GAS_LIMIT: U256 = U256::from(100_000);

    /*
        Comment by scx1332:
        In production I suggest limiting to 31.101 Gwei to not pay too much fees in extreme network situations
        Use:
        GLM_POLYGON_MAX_GAS_PRICE=31101000000
    */
    pub static ref GLM_POLYGON_MAX_GAS_PRICE: u64 =
        match env::var("GLM_POLYGON_MAX_GAS_PRICE").map(|s| s.parse()) {
            Ok(Ok(x)) => x,
            _ => 1000000000000, //1000 Gwei
        };
    /*
        Comment by scx1332:
        30.101 Gwei (as of 2021-10-08 30GWEI is minimum accepted gas price,
        network is under-utilised right now so 30.1 should result in express transactions
        USD cost as of 2021-10-08 of transaction is about 0,16 cents (USD)
    */

    pub static ref GLM_POLYGON_MIN_GAS_PRICE: u64 =
        match env::var("GLM_POLYGON_MIN_GAS_PRICE").map(|s| s.parse()) {
            Ok(Ok(x)) => x,
            _ => 30101000000,
    };
}
const CREATE_FAUCET_FUNCTION: &str = "create";
const BALANCE_ERC20_FUNCTION: &str = "balanceOf";
const TRANSFER_ERC20_FUNCTION: &str = "transfer";

pub async fn get_glm_balance(address: H160, network: Network) -> Result<U256, GenericError> {
    let client = get_client(network)?;
    let env = get_env(network)?;

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
        .map_err(GenericError::new)
}

pub async fn get_balance(address: H160, network: Network) -> Result<U256, GenericError> {
    let client = get_client(network)?;
    Ok(client
        .eth()
        .balance(address, None)
        .await
        .map_err(GenericError::new)?)
}

pub async fn get_next_nonce_pending(address: H160, network: Network) -> Result<U256, GenericError> {
    let client = get_client(network)?;
    let nonce = client
        .eth()
        .transaction_count(address, Some(web3::types::BlockNumber::Pending))
        .await
        .map_err(GenericError::new)?;
    Ok(nonce)
}

pub async fn block_number(network: Network) -> Result<U64, GenericError> {
    let client = get_client(network)?;
    Ok(client
        .eth()
        .block_number()
        .await
        .map_err(GenericError::new)?)
}

pub async fn sign_faucet_tx(
    address: H160,
    network: Network,
    nonce: U256,
) -> Result<TransactionEntity, GenericError> {
    let env = get_env(network)?;
    let client = get_client(network)?;
    let contract = prepare_glm_faucet_contract(&client, &env)?;
    let contract = match contract {
        Some(c) => c,
        None => {
            return Err(GenericError::new(
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
    let chain_id = network as u64;
    let node_id = NodeId::from(address.as_ref());
    let signature = bus::sign(node_id, eth_utils::get_tx_hash(&tx, chain_id)).await?;

    Ok(raw_tx_to_entity(
        &tx,
        address,
        chain_id,
        Utc::now(),
        &signature,
        TxType::Faucet,
    ))
}

pub async fn sign_transfer_tx(
    address: H160,
    recipient: H160,
    amount: U256,
    network: Network,
    nonce: U256,
    gas_price_override: Option<U256>,
    gas_limit_override: Option<u32>,
) -> Result<TransactionEntity, GenericError> {
    let env = get_env(network)?;
    let client = get_client(network)?;
    let contract = prepare_erc20_contract(&client, &env)?;

    let data = eth_utils::contract_encode(&contract, TRANSFER_ERC20_FUNCTION, (recipient, amount))
        .map_err(GenericError::new)?;
    let mut gas_price = client.eth().gas_price().await.map_err(GenericError::new)?;

    match network {
        Network::PolygonMainnet | Network::PolygonMumbai => {
            if gas_price < U256::from(*GLM_POLYGON_MIN_GAS_PRICE) {
                log::info!(
                    "Gas price lower than mininimum {}/{}. Continuing with higher gas price...",
                    gas_price,
                    *GLM_POLYGON_MIN_GAS_PRICE
                );
                gas_price = U256::from(*GLM_POLYGON_MIN_GAS_PRICE);
            }
            if let Some(gas_price_override) = gas_price_override {
                log::info!(
                    "Overriding gas price value new value: {} old value: {}",
                    gas_price_override,
                    gas_price
                );
                gas_price = gas_price_override;
            }
            if gas_price > U256::from(*GLM_POLYGON_MAX_GAS_PRICE) {
                log::warn!(
                    "Gas price higher than maximum {}/{}. Continuing with lower gas price...",
                    gas_price,
                    *GLM_POLYGON_MAX_GAS_PRICE
                );
                gas_price = U256::from(*GLM_POLYGON_MAX_GAS_PRICE);
            };
        }
        Network::Mainnet | Network::Rinkeby => {
            log::info!("Gas limits not implemented for Mainnet and Rinkeby networks",);
        }
    }

    let gas_limit = match gas_limit_override {
        Some(gas_limit_override) => U256::from(gas_limit_override),
        None => *GLM_POLYGON_GAS_LIMIT,
    };

    let tx = YagnaRawTransaction {
        nonce,
        to: Some(contract.address()),
        value: U256::from(0),
        gas_price,
        gas: gas_limit,
        data,
    };
    let chain_id = network as u64;
    let node_id = NodeId::from(address.as_ref());
    let signature = bus::sign(node_id, eth_utils::get_tx_hash(&tx, chain_id)).await?;

    Ok(raw_tx_to_entity(
        &tx,
        address,
        chain_id,
        Utc::now(),
        &signature,
        TxType::Transfer,
    ))
}

pub async fn send_tx(signed_tx: Vec<u8>, network: Network) -> Result<H256, GenericError> {
    let client = get_client(network)?;
    let tx_hash = client
        .eth()
        .send_raw_transaction(Bytes::from(signed_tx))
        .await
        .map_err(GenericError::new)?;
    Ok(tx_hash)
}

pub struct TransactionChainStatus {
    pub exists_on_chain: bool,
    pub pending: bool,
    pub confirmed: bool,
    pub succeeded: bool,
}

pub async fn get_tx_on_chain_status(
    tx_hash: H256,
    current_block: &U64,
    network: Network,
) -> Result<TransactionChainStatus, GenericError> {
    let mut res = TransactionChainStatus {
        exists_on_chain: false,
        pending: false,
        confirmed: false,
        succeeded: false,
    };
    let env = get_env(network)?;
    let tx = get_tx_receipt(tx_hash, network).await?;
    if let Some(tx) = tx {
        res.exists_on_chain = true;
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
                current_block
            );
            // tx.block_number is the first confirmation, so we need to - 1
            if tx_bn + env.required_confirmations - 1 <= *current_block {
                res.confirmed = true;
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
/*
pub async fn is_tx_succeeded(
    tx_hash: H256,
    current_block: &U64,
    network: Network,
) -> Result<bool, GenericError> {
    let env = get_env(network)?;
    let tx = get_tx_receipt(tx_hash, network).await?;
    if let Some(tx) = tx {
        if (tx.status == 1)
        {

        }
        if let Some(tx_bn) = tx.block_number {
            // TODO: Store tx.block_number in DB and check only once after required_confirmations.
            log::trace!(
                "is_tx_confirmed? tb + rq - 1 <= cb. tb={}, rq={}, cb={}",
                tx_bn,
                env.required_confirmations,
                current_block
            );
            // tx.block_number is the first confirmation, so we need to - 1
            if tx_bn + env.required_confirmations - 1 <= *current_block {
                return Ok(true);
            }
        }
    }
    Ok(false)
}*/

pub fn decode_encoded_transaction_data(
    network: Network,
    encoded: &str,
) -> Result<(ethereum_types::Address, ethereum_types::U256), GenericError> {
    let env = get_env(network)?;
    let client = get_client(network)?;
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
    Err(GenericError::new("Failed to parse tokens"))
}

pub async fn get_tx_from_network(
    tx_hash: H256,
    network: Network,
) -> Result<Option<Transaction>, GenericError> {
    let client = get_client(network)?;
    let result = client
        .eth()
        .transaction(TransactionId::from(tx_hash))
        .await
        .map_err(GenericError::new)?;
    Ok(result)
}

pub async fn get_tx_receipt(
    tx_hash: H256,
    network: Network,
) -> Result<Option<TransactionReceipt>, GenericError> {
    let client = get_client(network)?;
    let result = client
        .eth()
        .transaction_receipt(tx_hash)
        .await
        .map_err(GenericError::new)?;
    Ok(result)
}

fn get_rpc_addr_from_env(network: Network) -> Result<String, GenericError> {
    match network {
        Network::Mainnet => Ok(std::env::var("MAINNET_GETH_ADDR")
            .unwrap_or("https://geth.golem.network:55555".to_string())),
        Network::Rinkeby => Ok(std::env::var("RINKEBY_GETH_ADDR")
            .unwrap_or("http://geth.testnet.golem.network:55555".to_string())),
        Network::PolygonMainnet => Ok(std::env::var("POLYGON_MAINNET_GETH_ADDR")
            .unwrap_or("https://bor.golem.network".to_string())),
        Network::PolygonMumbai => Ok(std::env::var("POLYGON_MUMBAI_GETH_ADDR").unwrap_or(
            "https://polygon-mumbai.infura.io/v3/4dfe7a7afc6d4549b16490db5fd6358e".to_string(),
        )),
    }
}

fn get_client(network: Network) -> Result<Web3<Http>, GenericError> {
    let geth_addr = get_rpc_addr_from_env(network)?;

    let transport = web3::transports::Http::new(&geth_addr).map_err(GenericError::new)?;

    Ok(Web3::new(transport))
}

fn get_env(network: Network) -> Result<config::EnvConfiguration, GenericError> {
    match network {
        Network::Mainnet => Ok(*config::MAINNET_CONFIG),
        Network::Rinkeby => Ok(*config::RINKEBY_CONFIG),
        Network::PolygonMumbai => Ok(*config::MUMBAI_CONFIG),
        Network::PolygonMainnet => Ok(*config::POLYGON_MAINNET_CONFIG),
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

fn raw_tx_to_entity(
    raw_tx: &YagnaRawTransaction,
    sender: H160,
    chain_id: u64,
    timestamp: DateTime<Utc>,
    signature: &Vec<u8>,
    tx_type: TxType,
) -> TransactionEntity {
    TransactionEntity {
        tx_id: prepare_tx_id(&raw_tx, chain_id, sender),
        sender: format!("0x{:x}", sender),
        nonce: base_utils::u256_to_big_endian_hex(raw_tx.nonce),
        timestamp: timestamp.naive_utc(),
        encoded: serde_json::to_string(raw_tx).unwrap(),
        status: TransactionStatus::Created as i32,
        tx_type: tx_type as i32,
        signature: hex::encode(signature),
        tx_hash: None,
        network: Network::from_u64(chain_id).unwrap(),
    }
}

// We need a function to prepare an unique identifier for tx
// that could be calculated easily from RawTransaction data
// Explanation: RawTransaction::hash() can produce the same output (sender does not have any impact)
pub fn prepare_tx_id(raw_tx: &YagnaRawTransaction, chain_id: u64, sender: H160) -> String {
    let mut bytes = eth_utils::get_tx_hash(raw_tx, chain_id);
    let mut address = sender.as_bytes().to_vec();
    bytes.append(&mut address);
    // TODO: Try https://docs.rs/web3/0.13.0/web3/api/struct.Web3Api.html#method.sha3
    format!("{:x}", Sha3_512::digest(&bytes))
}

pub fn get_max_gas_costs(db_tx: &TransactionEntity) -> Result<U256, GenericError> {
    let raw_tx: YagnaRawTransaction =
        serde_json::from_str(&db_tx.encoded).map_err(GenericError::new)?;
    Ok(raw_tx.gas_price * raw_tx.gas)
}
