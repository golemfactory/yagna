use crate::gnt::config;
use crate::gnt::ethereum::EthereumClient;
use crate::{utils, GNTDriverError, GNTDriverResult};
use bigdecimal::BigDecimal;
use ethereum_types::{Address, H256, U256, U64};
use futures3::compat::*;
use web3::contract::{Contract, Options};
use web3::transports::Http;
use web3::types::{Bytes, Log, TransactionReceipt};
use web3::Transport;
use ya_core_model::driver::PaymentDetails;

pub(crate) fn prepare_gnt_contract(
    ethereum_client: &EthereumClient,
    env: &config::EnvConfiguration,
) -> GNTDriverResult<Contract<Http>> {
    prepare_contract(
        ethereum_client,
        env.gnt_contract_address,
        include_bytes!("../contracts/gnt2.json"),
    )
}

fn prepare_contract(
    ethereum_client: &EthereumClient,
    address: Address,
    json_abi: &[u8],
) -> GNTDriverResult<Contract<Http>> {
    let contract = ethereum_client.get_contract(address, json_abi)?;
    Ok(contract)
}

pub(crate) fn prepare_gnt_faucet_contract(
    ethereum_client: &EthereumClient,
    env: &config::EnvConfiguration,
) -> GNTDriverResult<Option<Contract<Http>>> {
    if let Some(gnt_faucet_address) = env.gnt_faucet_address {
        Ok(Some(prepare_contract(
            ethereum_client,
            gnt_faucet_address,
            include_bytes!("../contracts/faucet.json"),
        )?))
    } else {
        Ok(None)
    }
}

pub(crate) async fn get_eth_balance(
    ethereum_client: &EthereumClient,
    address: Address,
) -> GNTDriverResult<BigDecimal> {
    let block_number = None;
    let amount = ethereum_client
        .get_eth_balance(address, block_number)
        .await?;
    Ok(utils::u256_to_big_dec(amount)?)
}

pub(crate) async fn get_gnt_balance(
    gnt_contract: &Contract<Http>,
    address: Address,
) -> GNTDriverResult<BigDecimal> {
    gnt_contract
        .query("balanceOf", (address,), None, Options::default(), None)
        .compat()
        .await
        .map_or_else(
            |e| Err(GNTDriverError::LibraryError(format!("{:?}", e))),
            |amount| Ok(utils::u256_to_big_dec(amount)?),
        )
}

pub(crate) fn verify_gnt_tx<T: Transport>(
    receipt: &TransactionReceipt,
    contract: &Contract<T>,
) -> GNTDriverResult<()> {
    verify_gnt_tx_logs(&receipt.logs, contract)?;
    verify_gnt_tx_status(&receipt.status)?;
    Ok(())
}

fn verify_gnt_tx_status(status: &Option<U64>) -> GNTDriverResult<()> {
    match status {
        None => Err(GNTDriverError::UnknownTransaction),
        Some(status) => {
            if *status == U64::from(config::ETH_TX_SUCCESS) {
                Ok(())
            } else {
                Err(GNTDriverError::FailedTransaction)
            }
        }
    }
}

fn verify_gnt_tx_logs<T: Transport>(
    logs: &Vec<Log>,
    contract: &Contract<T>,
) -> GNTDriverResult<()> {
    if logs.len() != config::TRANSFER_LOGS_LENGTH {
        return Err(GNTDriverError::UnknownTransaction);
    }
    verify_gnt_tx_log(&logs[0], contract)?;
    Ok(())
}

fn verify_gnt_tx_log<T: Transport>(log: &Log, contract: &Contract<T>) -> GNTDriverResult<()> {
    if log.address != contract.address() {
        return Err(GNTDriverError::UnknownTransaction);
    }
    verify_gnt_tx_log_topics(&log.topics)?;
    verify_gnt_tx_log_data(&log.data)?;
    Ok(())
}

fn verify_gnt_tx_log_topics(topics: &Vec<H256>) -> GNTDriverResult<()> {
    if topics.len() != config::TX_LOG_TOPICS_LENGTH {
        return Err(GNTDriverError::UnknownTransaction);
    }
    // topics[0] is the keccak-256 of the Transfer(address,address,uint256) canonical signature
    verify_gnt_tx_log_canonical_signature(&topics[0])?;
    Ok(())
}

fn verify_gnt_tx_log_canonical_signature(canonical_signature: &H256) -> GNTDriverResult<()> {
    if *canonical_signature
        != H256::from_slice(&hex::decode(config::TRANSFER_CANONICAL_SIGNATURE).unwrap())
    {
        return Err(GNTDriverError::UnknownTransaction);
    }
    Ok(())
}

fn verify_gnt_tx_log_data(data: &Bytes) -> GNTDriverResult<()> {
    if data.0.len() != config::TX_LOG_DATA_LENGTH {
        return Err(GNTDriverError::UnknownTransaction);
    }
    Ok(())
}

pub(crate) fn build_payment_details(
    receipt: &TransactionReceipt,
) -> GNTDriverResult<PaymentDetails> {
    // topics[1] is the value of the _from address as H256
    let sender = utils::topic_to_address(&receipt.logs[0].topics[1]);
    // topics[2] is the value of the _to address as H256
    let recipient = utils::topic_to_address(&receipt.logs[0].topics[2]);
    // The data field from the returned Log struct contains the transferred token amount value
    let amount: U256 = utils::u256_from_big_endian(&receipt.logs[0].data.0);
    // Do not have any info about date in receipt
    let date = None;

    Ok(PaymentDetails {
        recipient: utils::addr_to_str(recipient).into(),
        sender: utils::addr_to_str(sender).into(),
        amount: utils::u256_to_big_dec(amount)?,
        date,
    })
}
