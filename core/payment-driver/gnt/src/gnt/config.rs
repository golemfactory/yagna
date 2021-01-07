use crate::gnt::ethereum::Chain;
use crate::{utils, GNTDriverError, GNTDriverResult};
use ethereum_types::{Address, H160};

use std::env;

pub(crate) const MAX_TESTNET_BALANCE: &str = "1000";

pub(crate) const ETH_TX_SUCCESS: u64 = 1;
pub(crate) const TRANSFER_LOGS_LENGTH: usize = 1;
pub(crate) const TX_LOG_DATA_LENGTH: usize = 32;
pub(crate) const TX_LOG_TOPICS_LENGTH: usize = 3;
pub(crate) const TRANSFER_CANONICAL_SIGNATURE: &str =
    "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

pub(crate) const NGNT_CONTRACT_ADDRESS_ENV_VAR: &str = "NGNT_CONTRACT_ADDRESS";
pub(crate) const NGNT_FAUCET_CONTRACT_ADDRESS_ENV_VAR: &str = "NGNT_FAUCET_CONTRACT_ADDRESS";
pub(crate) const REQUIRED_CONFIRMATIONS_ENV_VAR: &str = "REQUIRED_CONFIRMATIONS";

pub struct EnvConfiguration {
    pub gnt_contract_address: Address,
    pub gnt_faucet_address: Option<Address>,
    pub required_confirmations: u64,
}

pub const CFG_TESTNET: EnvConfiguration = EnvConfiguration {
    gnt_contract_address: H160([
        0xd9, 0x4e, 0x3D, 0xC3, 0x9d, 0x4C, 0xad, 0x1D, 0xAd, 0x63, 0x4e, 0x7e, 0xb5, 0x85, 0xA5,
        0x7A, 0x19, 0xdC, 0x7E, 0xFE,
    ]),
    gnt_faucet_address: Some(H160([
        0x59, 0x25, 0x99, 0x43, 0x61, 0x62, 0x65, 0xA0, 0x3d, 0x77, 0x51, 0x45, 0xa2, 0xeC, 0x37,
        0x17, 0x32, 0xE2, 0xB0, 0x6C,
    ])),
    required_confirmations: 1,
};

pub const CFG_MAINNET: EnvConfiguration = EnvConfiguration {
    gnt_contract_address: H160([
        0x7D, 0xD9, 0xc5, 0xCb, 0xa0, 0x5E, 0x15, 0x1C, 0x89, 0x5F, 0xDe, 0x1C, 0xF3, 0x55, 0xC9,
        0xA1, 0xD5, 0xDA, 0x64, 0x29,
    ]),
    gnt_faucet_address: None,
    required_confirmations: 5,
};

impl EnvConfiguration {
    pub fn from_env(chain: Chain) -> GNTDriverResult<Self> {
        let mut base = match chain {
            Chain::Rinkeby => CFG_TESTNET,
            Chain::Mainnet => CFG_MAINNET,
        };
        if let Some(gnt_contract_address) = env::var(NGNT_CONTRACT_ADDRESS_ENV_VAR).ok() {
            base.gnt_contract_address = utils::str_to_addr(&gnt_contract_address)?;
        }
        if let Some(gnt_faucet_address) = env::var(NGNT_FAUCET_CONTRACT_ADDRESS_ENV_VAR).ok() {
            base.gnt_faucet_address = Some(utils::str_to_addr(&gnt_faucet_address)?);
        }
        if let Some(required_confirmations) = env::var(REQUIRED_CONFIRMATIONS_ENV_VAR).ok() {
            base.required_confirmations = required_confirmations.parse().map_err(|_| {
                GNTDriverError::library_err_msg(format!(
                    "invalid {} value: {}",
                    REQUIRED_CONFIRMATIONS_ENV_VAR, required_confirmations
                ))
            })?;
        }
        Ok(base)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_address() {
        assert_eq!(
            CFG_TESTNET.gnt_contract_address,
            utils::str_to_addr("0xd94e3DC39d4Cad1DAd634e7eb585A57A19dC7EFE").unwrap()
        );
        assert_eq!(
            CFG_TESTNET.gnt_faucet_address.unwrap(),
            utils::str_to_addr("0x59259943616265A03d775145a2eC371732E2B06C").unwrap()
        )
    }
}
