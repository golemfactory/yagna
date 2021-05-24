/*
    Erc20 related utilities.
*/

use std::str::FromStr;

// External uses
use bigdecimal::BigDecimal;
use ethereum_types::{Address, H160, H256, U256};
use lazy_static::lazy_static;
use num_bigint::{BigInt, BigUint, ToBigInt};

// Workspace uses
use ya_payment_driver::model::GenericError;

lazy_static! {
    // TODO: Get token decimals from erc20-provider / wallet
    pub static ref PRECISION: BigDecimal = BigDecimal::from(1_000_000_000_000_000_000u64);
}

pub fn big_dec_to_u256(v: BigDecimal) -> Result<U256, GenericError> {
    let v = v * &(*PRECISION);
    let v = v
        .to_bigint()
        .ok_or(GenericError::new("Failed to convert to bigint"))?;
    let v = &v.to_string();
    Ok(U256::from_dec_str(v).map_err(GenericError::new)?)
}

pub fn u256_to_big_dec(v: U256) -> Result<BigDecimal, GenericError> {
    let v: BigDecimal = v.to_string().parse().map_err(GenericError::new)?;
    Ok(v / &(*PRECISION))
}

pub fn big_uint_to_big_dec(v: BigUint) -> BigDecimal {
    let v: BigDecimal = Into::<BigInt>::into(v).into();
    v / &(*PRECISION)
}

pub fn topic_to_str_address(topic: &H256) -> String {
    let result = H160::from_slice(&topic.as_bytes()[12..]);
    format!("0x{:x}", result)
}

pub fn str_to_big_dec(v: &str) -> Result<BigDecimal, GenericError> {
    let v: BigDecimal = BigDecimal::from_str(v).map_err(GenericError::new)?;
    Ok(v)
}

pub fn str_to_addr(addr: &str) -> Result<Address, GenericError> {
    match addr.trim_start_matches("0x").parse() {
        Ok(addr) => Ok(addr),
        Err(_e) => Err(GenericError::new(format!(
            "Unable to parse addres {}",
            addr.to_string()
        ))),
    }
}
