use crate::utils;
use ethereum_types::Address;
use lazy_static::*;
use std::env;

pub(crate) const GNT_TRANSFER_GAS: u32 = 55000;
pub(crate) const GNT_FAUCET_GAS: u32 = 90000;

pub(crate) const MAX_TESTNET_BALANCE: &str = "1000";

pub(crate) const ETH_TX_SUCCESS: u64 = 1;
pub(crate) const TRANSFER_LOGS_LENGTH: usize = 1;
pub(crate) const TX_LOG_DATA_LENGTH: usize = 32;
pub(crate) const TX_LOG_TOPICS_LENGTH: usize = 3;
pub(crate) const TRANSFER_CANONICAL_SIGNATURE: &str =
    "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

pub(crate) const TX_SENDER_BUFFER: usize = 100;

pub(crate) const GNT_CONTRACT_ADDRESS_ENV_KEY: &str = "GNT_CONTRACT_ADDRESS";
pub(crate) const GNT_FAUCET_CONTRACT_ADDRESS_ENV_KEY: &str = "GNT_FAUCET_CONTRACT_ADDRESS";
pub(crate) const REQUIRED_CONFIRMATIONS_ENV_KEY: &str = "REQUIRED_CONFIRMATIONS";

lazy_static! {
    pub static ref GNT_CONTRACT_ADDRESS: Address = utils::str_to_addr(
        env::var(GNT_CONTRACT_ADDRESS_ENV_KEY)
            .expect(format!("Missing {} env variable...", GNT_CONTRACT_ADDRESS_ENV_KEY).as_str())
            .as_str()
    )
    .unwrap();
    pub static ref GNT_FAUCET_CONTRACT_ADDRESS: Address = utils::str_to_addr(
        env::var(GNT_FAUCET_CONTRACT_ADDRESS_ENV_KEY)
            .expect(
                format!(
                    "Missing {} env variable...",
                    GNT_FAUCET_CONTRACT_ADDRESS_ENV_KEY
                )
                .as_str()
            )
            .as_str()
    )
    .unwrap();
    pub static ref REQUIRED_CONFIRMATIONS: usize = env::var(REQUIRED_CONFIRMATIONS_ENV_KEY)
        .expect(format!("Missing {} env variable...", REQUIRED_CONFIRMATIONS_ENV_KEY).as_str())
        .parse()
        .expect(
            format!(
                "Incorrect {} env variable...",
                REQUIRED_CONFIRMATIONS_ENV_KEY
            )
            .as_str()
        );
}
