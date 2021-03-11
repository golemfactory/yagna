/*
    Erc20 related utilities.
*/

// External uses
use bigdecimal::BigDecimal;
use lazy_static::lazy_static;
use num_bigint::{BigInt, BigUint, ToBigInt};

// Workspace uses
use ya_payment_driver::model::GenericError;

lazy_static! {
    // TODO: Get token decimals from erc20-provider / wallet
    pub static ref PRECISION: BigDecimal = BigDecimal::from(1_000_000_000_000_000_000u64);
}

pub fn big_dec_to_big_uint(v: BigDecimal) -> Result<BigUint, GenericError> {
    let v = v * &(*PRECISION);
    let v = v
        .to_bigint()
        .ok_or(GenericError::new("Failed to convert to bigint"))?;
    let v = v
        .to_biguint()
        .ok_or(GenericError::new("Failed to convert to biguint"))?;
    Ok(v)
}

pub fn big_uint_to_big_dec(v: BigUint) -> BigDecimal {
    let v: BigDecimal = Into::<BigInt>::into(v).into();
    v / &(*PRECISION)
}
