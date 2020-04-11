use crate::error::PaymentDriverError;
use crate::PaymentDriverResult;

use bigdecimal::BigDecimal;
use ethereum_types::{Address, U256};
use num_bigint::ToBigInt;
use std::str::FromStr;

const PRECISION: u64 = 1_000_000_000_000_000_000;
pub fn str_to_addr(addr: &str) -> PaymentDriverResult<Address> {
    match addr.trim_start_matches("0x").parse() {
        Ok(addr) => Ok(addr),
        Err(_e) => Err(PaymentDriverError::Address(addr.to_string())),
    }
}

pub fn addr_to_str(addr: Address) -> String {
    format!("0x{}", hex::encode(addr.to_fixed_bytes()))
}

pub fn big_dec_to_u256(v: BigDecimal) -> PaymentDriverResult<U256> {
    let v = v * Into::<BigDecimal>::into(PRECISION);
    let v = v.to_bigint().unwrap();
    let v = &v.to_string();
    Ok(U256::from_dec_str(v)?)
}

pub fn u256_to_big_dec(v: U256) -> PaymentDriverResult<BigDecimal> {
    let v: BigDecimal = v.to_string().parse()?;
    Ok(v / Into::<BigDecimal>::into(PRECISION))
}

pub fn str_to_big_dec(v: &str) -> PaymentDriverResult<BigDecimal> {
    let v: BigDecimal = BigDecimal::from_str(v)?;
    Ok(v)
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
