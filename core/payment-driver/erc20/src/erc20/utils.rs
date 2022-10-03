/*
    Erc20 related utilities.
*/

use std::str::FromStr;

// External uses
use bigdecimal::BigDecimal;
use lazy_static::lazy_static;
use num_bigint::{BigInt, BigUint, ToBigInt};
use web3::types::{Address, H160, H256, U256};
// Workspace uses
use ya_payment_driver::model::GenericError;

lazy_static! {
    // TODO: Get token decimals from erc20-provider / wallet
    pub static ref PRECISION: BigDecimal = BigDecimal::from(1_000_000_000_000_000_000u64);
    pub static ref GWEI_PRECISION: BigDecimal = BigDecimal::from(1_000_000_000u64);
}

pub fn big_dec_to_u256(v: &BigDecimal) -> Result<U256, GenericError> {
    let v = v * &(*PRECISION);
    let v = v
        .to_bigint()
        .ok_or_else(|| GenericError::new("Failed to convert to bigint"))?;
    let v = &v.to_string();
    Ok(U256::from_dec_str(v).map_err(GenericError::new)?)
}

pub fn big_dec_gwei_to_u256(v: BigDecimal) -> Result<U256, GenericError> {
    let v = v * &(*GWEI_PRECISION);
    let v = v
        .to_bigint()
        .ok_or_else(|| GenericError::new("Failed to convert to bigint"))?;
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
            "Unable to parse address {}",
            addr
        ))),
    }
}

pub fn convert_float_gas_to_u256(gas_in_gwei: f64) -> U256 {
    let gas_in_wei = gas_in_gwei * 1.0E9;
    let gas_in_wei_int = gas_in_wei as u64;
    U256::from(gas_in_wei_int)
}
pub fn convert_u256_gas_to_float(gas_in_wei: U256) -> f64 {
    let gas_in_wei = gas_in_wei.as_u64() as f64;

    gas_in_wei * 1.0E-9
}

pub fn gas_float_equals(gas_value1: f64, gas_value2: f64) -> bool {
    if gas_value1 > 0.0
        && gas_value2 > 0.0
        && (gas_value1 - gas_value2).abs() / (gas_value1 + gas_value2) < 0.0001
    {
        return true;
    }
    if gas_value1 == 0.0 && gas_value2 == 0.0 {
        return true;
    }
    false
}
