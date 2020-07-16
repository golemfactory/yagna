use crate::error::GNTDriverError;
use crate::GNTDriverResult;

use crate::models::{TransactionEntity, TransactionStatus, TxType};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use ethereum_tx_sign::RawTransaction;
use ethereum_types::{Address, H160, H256, U256};
use futures3::{Future, FutureExt};
use num_bigint::ToBigInt;
use sha3::{Digest, Sha3_512};
use std::pin::Pin;
use std::str::FromStr;
use ya_client_model::NodeId;
use ya_core_model::identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

const PRECISION: u64 = 1_000_000_000_000_000_000;

pub const PAYMENT_STATUS_NOT_YET: i32 = 1;
pub const PAYMENT_STATUS_OK: i32 = 2;
pub const PAYMENT_STATUS_NOT_ENOUGH_FUNDS: i32 = 3;
pub const PAYMENT_STATUS_NOT_ENOUGH_GAS: i32 = 4;
pub const PAYMENT_STATUS_FAILED: i32 = 5;

pub fn str_to_addr(addr: &str) -> GNTDriverResult<Address> {
    match addr.trim_start_matches("0x").parse() {
        Ok(addr) => Ok(addr),
        Err(_e) => Err(GNTDriverError::Address(addr.to_string())),
    }
}

pub fn addr_to_str(addr: impl std::borrow::Borrow<Address>) -> String {
    format!("0x{}", hex::encode(addr.borrow()))
}

pub fn big_dec_to_u256(v: BigDecimal) -> GNTDriverResult<U256> {
    let v = v * Into::<BigDecimal>::into(PRECISION);
    let v = v.to_bigint().unwrap();
    let v = &v.to_string();
    Ok(U256::from_dec_str(v)?)
}

pub fn u256_to_big_dec(v: U256) -> GNTDriverResult<BigDecimal> {
    let v: BigDecimal = v.to_string().parse()?;
    Ok(v / Into::<BigDecimal>::into(PRECISION))
}

pub fn str_to_big_dec(v: &str) -> GNTDriverResult<BigDecimal> {
    let v: BigDecimal = BigDecimal::from_str(v)?;
    Ok(v)
}

pub fn topic_to_address(topic: &H256) -> Address {
    H160::from_slice(&topic.as_bytes()[12..])
}

pub fn u256_from_big_endian(bytes: &Vec<u8>) -> U256 {
    U256::from_big_endian(bytes)
}

pub fn u256_to_big_endian_hex(value: U256) -> String {
    let mut bytes = [0u8; 32];
    value.to_big_endian(&mut bytes);
    hex::encode(&bytes)
}

pub fn u256_from_big_endian_hex(bytes: String) -> U256 {
    let bytes = hex::decode(&bytes).unwrap();
    U256::from_big_endian(&bytes)
}

pub fn h256_from_hex(bytes: String) -> H256 {
    let bytes = hex::decode(&bytes).unwrap();
    H256::from_slice(&bytes)
}

pub fn raw_tx_to_entity(
    raw_tx: &RawTransaction,
    sender: Address,
    chain_id: u64,
    timestamp: DateTime<Utc>,
    signature: &Vec<u8>,
    tx_type: TxType,
) -> TransactionEntity {
    TransactionEntity {
        tx_id: prepare_tx_id(&raw_tx, chain_id, sender),
        sender: addr_to_str(sender),
        nonce: u256_to_big_endian_hex(raw_tx.nonce),
        timestamp: timestamp.naive_utc(),
        encoded: serde_json::to_string(raw_tx).unwrap(),
        status: TransactionStatus::Created.into(),
        tx_type: tx_type.into(),
        signature: hex::encode(signature),
        tx_hash: None,
    }
}

// We need a function to prepare an unique identifier for tx
// that could be calculated easily from RawTransaction data
// Explanation: RawTransaction::hash() can produce the same output (sender does not have any impact)
pub fn prepare_tx_id(raw_tx: &RawTransaction, chain_id: u64, sender: Address) -> String {
    let mut bytes = raw_tx.hash(chain_id);
    let mut address = sender.as_bytes().to_vec();
    bytes.append(&mut address);
    format!("{:x}", Sha3_512::digest(&bytes))
}

pub fn get_sign_tx(node_id: NodeId) -> impl Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>> {
    move |payload| {
        let fut = bus::service(identity::BUS_ID)
            .send(identity::Sign { node_id, payload })
            .map(|x| x.unwrap().unwrap());
        Box::pin(fut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_conversion() {
        let amount_str = "10000.123456789012345678";
        let big_dec = str_to_big_dec(&amount_str).unwrap();
        let u256 = big_dec_to_u256(big_dec.clone()).unwrap();
        assert_eq!(big_dec, u256_to_big_dec(u256).unwrap());
    }

    #[test]
    fn test_address_conversion() {
        let addr_str = "0xd39a168f0480b8502c2531b2ffd8588c592d713a";
        let addr = str_to_addr(addr_str).unwrap();
        assert_eq!(addr_str, addr_to_str(addr));
    }
}
