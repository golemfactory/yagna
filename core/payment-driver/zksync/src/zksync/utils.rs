/*
    Zksync related utilities.
*/

// External uses
use bigdecimal::BigDecimal;
use lazy_static::lazy_static;
use num_bigint::{BigInt, BigUint, ToBigInt};
use zksync::utils::{closest_packable_token_amount, is_token_amount_packable};

// Workspace uses
use ya_payment_driver::model::GenericError;

lazy_static! {
    // TODO: Get token decimals from zksync-provider / wallet
    pub static ref PRECISION: BigDecimal = BigDecimal::from(1_000_000_000_000_000_000u64);
}

pub fn big_dec_to_big_uint(v: BigDecimal) -> Result<BigUint, GenericError> {
    let v = v * &(*PRECISION);
    let v = v
        .to_bigint()
        .ok_or_else(|| GenericError::new("Failed to convert to bigint"))?;
    let v = v
        .to_biguint()
        .ok_or_else(|| GenericError::new("Failed to convert to biguint"))?;
    Ok(v)
}

pub fn big_uint_to_big_dec(v: BigUint) -> BigDecimal {
    let v: BigDecimal = Into::<BigInt>::into(v).into();
    v / &(*PRECISION)
}

/// Find the closest **bigger** packable amount
pub fn pack_up(amount: &BigUint) -> BigUint {
    let mut packable_amount = closest_packable_token_amount(&amount);
    while (&packable_amount < amount) || !is_token_amount_packable(&packable_amount) {
        packable_amount = increase_least_significant_digit(&packable_amount);
    }
    packable_amount
}

fn increase_least_significant_digit(amount: &BigUint) -> BigUint {
    let digits = amount.to_radix_le(10);
    for i in 0..digits.len() {
        if digits[i] != 0 {
            return amount + BigUint::from(10u32).pow(i as u32);
        }
    }
    amount.clone() // zero
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_increase_least_significant_digit() {
        let amount = BigUint::from_str("999000").unwrap();
        let increased = increase_least_significant_digit(&amount);
        let expected = BigUint::from_str("1000000").unwrap();
        assert_eq!(increased, expected);
    }

    #[test]
    fn test_pack_up() {
        let amount = BigUint::from_str("12300285190700000000").unwrap();
        let packable = pack_up(&amount);
        assert!(
            zksync::utils::is_token_amount_packable(&packable),
            "Not packable!"
        );
        assert!(packable >= amount, "To little!");
    }
}
