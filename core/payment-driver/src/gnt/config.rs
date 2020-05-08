use crate::ethereum::Chain;
use crate::{utils, PaymentDriverResult};
use ethereum_types::{Address, H160};
use lazy_static::*;
use std::env;

pub(crate) const MAX_TESTNET_BALANCE: &str = "1000";

pub(crate) const ETH_TX_SUCCESS: u64 = 1;
pub(crate) const TRANSFER_LOGS_LENGTH: usize = 1;
pub(crate) const TX_LOG_DATA_LENGTH: usize = 32;
pub(crate) const TX_LOG_TOPICS_LENGTH: usize = 3;
pub(crate) const TRANSFER_CANONICAL_SIGNATURE: &str =
    "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

pub(crate) const GNT_CONTRACT_ADDRESS_ENV_KEY: &str = "GNT_CONTRACT_ADDRESS";
pub(crate) const GNT_FAUCET_CONTRACT_ADDRESS_ENV_KEY: &str = "GNT_FAUCET_CONTRACT_ADDRESS";
pub(crate) const REQUIRED_CONFIRMATIONS_ENV_KEY: &str = "REQUIRED_CONFIRMATIONS";

pub struct EnvConfiguration {
    pub gnt_contract_address: Address,
    pub gnt_faucet_address: Option<Address>,
    pub required_confirmations: usize,
}

pub const CFG_TESTNET: EnvConfiguration = EnvConfiguration {
    gnt_contract_address: H160([
        0x92, 0x44, 0x42, 0xA6, 0x6c, 0xFd, 0x81, 0x23, 0x08, 0x79, 0x18, 0x72, 0xC4, 0xB2, 0x42,
        0x44, 0x0c, 0x10, 0x8E, 0x19,
    ]),
    gnt_faucet_address: Some(H160([
        0x77, 0xb6, 0x14, 0x5E, 0x85, 0x3d, 0xfA, 0x80, 0xE8, 0x75, 0x5a, 0x4e, 0x82, 0x4c, 0x4F,
        0x51, 0x0a, 0xc6, 0x69, 0x2e,
    ])),
    required_confirmations: 5,
};

pub const CFG_MAINNET: EnvConfiguration = EnvConfiguration {
    gnt_contract_address: H160([
        0xa7, 0x44, 0x76, 0x44, 0x31, 0x19, 0xA9, 0x42, 0xdE, 0x49, 0x85, 0x90, 0xFe, 0x1f, 0x24,
        0x54, 0xd7, 0xD4, 0xaC, 0x0d,
    ]),
    gnt_faucet_address: None,
    required_confirmations: 5,
};

impl EnvConfiguration {
    pub fn from_env(chain: Chain) -> PaymentDriverResult<Self> {
        let mut base = match chain {
            Chain::Rinkeby => CFG_TESTNET,
            Chain::Mainnet => CFG_MAINNET,
        };
        if let Some(gnt_contract_address) = env::var(GNT_FAUCET_CONTRACT_ADDRESS_ENV_KEY).ok() {
            base.gnt_contract_address = utils::str_to_addr(&gnt_contract_address)?;
        }
        if let Some(gnt_faucet_address) = env::var(GNT_FAUCET_CONTRACT_ADDRESS_ENV_KEY).ok() {
            base.gnt_faucet_address = Some(utils::str_to_addr(&gnt_faucet_address)?);
        }
        Ok(base)
    }
}

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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_address() {
        assert_eq!(
            CFG_TESTNET.gnt_contract_address,
            utils::str_to_addr("0x924442A66cFd812308791872C4B242440c108E19").unwrap()
        );
        assert_eq!(
            CFG_TESTNET.gnt_faucet_address.unwrap(),
            utils::str_to_addr("0x77b6145E853dfA80E8755a4e824c4F510ac6692e").unwrap()
        )
    }
}
