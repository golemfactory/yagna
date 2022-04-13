use lazy_static::lazy_static;
use std::env;
use web3::types::Address;

use crate::erc20::utils;

// TODO: REUSE old verification checks?
// pub(crate) const ETH_TX_SUCCESS: u64 = 1;
// pub(crate) const TRANSFER_LOGS_LENGTH: usize = 1;
// pub(crate) const TX_LOG_DATA_LENGTH: usize = 32;
// pub(crate) const TX_LOG_TOPICS_LENGTH: usize = 3;
// pub(crate) const TRANSFER_CANONICAL_SIGNATURE: &str =
//     "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

#[derive(Clone, Copy, Debug)]
pub struct EnvConfiguration {
    pub glm_contract_address: Address,
    pub glm_multi_transfer_contract_address: Option<Address>,
    pub glm_faucet_address: Option<Address>,
    pub required_confirmations: u64,
    pub payment_max_processed: u64,
    pub use_external_gas_provider: bool,
    pub latest_block_max_seconds_behind: i64,
}

pub const DEFAULT_MAX_PROCESSED: u64 = 30;

lazy_static! {
    pub static ref RINKEBY_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("RINKEBY_TGLM_CONTRACT_ADDRESS")
                .unwrap_or("0xd94e3DC39d4Cad1DAd634e7eb585A57A19dC7EFE".to_string())
        )
        .unwrap(),
        glm_multi_transfer_contract_address: Some(utils::str_to_addr(
            &env::var("RINKEBY_MULTI_TRANSFER_CONTRACT_ADDRESS")
                .unwrap_or("0x121F36E0Fa04fe6fBbEE84451F3371386B035fBA".to_string())
        )
        .unwrap()),
        glm_faucet_address: Some(
            utils::str_to_addr(
                &env::var("RINKEBY_TGLM_FAUCET_ADDRESS")
                    .unwrap_or("0x59259943616265A03d775145a2eC371732E2B06C".to_string())
            )
            .unwrap()
        ),
        required_confirmations: {
            match env::var("ERC20_RINKEBY_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        },
        payment_max_processed: {
            match env::var("ERC20_RINKEBY_PAYMENT_MAX_PROCESSED").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => DEFAULT_MAX_PROCESSED,
            }
        },
        latest_block_max_seconds_behind: {
            match env::var("ERC20_RINKEBY_LATEST_BLOCK_SECONDS_BEHIND").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 600,
            }
        },
        use_external_gas_provider: false
    };
    pub static ref MAINNET_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("MAINNET_GLM_CONTRACT_ADDRESS")
                .unwrap_or("0x7DD9c5Cba05E151C895FDe1CF355C9A1D5DA6429".to_string())
        )
        .unwrap(),
        glm_multi_transfer_contract_address: None,
        glm_faucet_address: None,
        required_confirmations: {
            match env::var("ERC20_MAINNET_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 5,
            }
        },
        payment_max_processed: {
            match env::var("ERC20_MAINNET_PAYMENT_MAX_PROCESSED").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => DEFAULT_MAX_PROCESSED,
            }
        },
        latest_block_max_seconds_behind: {
            match env::var("ERC20_MAINNET_LATEST_BLOCK_SECONDS_BEHIND").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 120,
            }
        },
        use_external_gas_provider: false
    };
    pub static ref GOERLI_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("GOERLI_TGLM_CONTRACT_ADDRESS")
                .unwrap_or("0x33af15c79d64b85ba14aaffaa4577949104b22e8".to_string())
        )
        .unwrap(),
        glm_multi_transfer_contract_address: None,
        glm_faucet_address: None,
        required_confirmations: {
            match env::var("ERC20_GOERLI_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        },
        payment_max_processed: {
            match env::var("ERC20_GOERLI_PAYMENT_MAX_PROCESSED").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => DEFAULT_MAX_PROCESSED,
            }
        },
        latest_block_max_seconds_behind: {
            match env::var("ERC20_GOERLI_LATEST_BLOCK_SECONDS_BEHIND").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 600,
            }
        },
        use_external_gas_provider: false
    };
    pub static ref MUMBAI_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("MUMBAI_TGLM_CONTRACT_ADDRESS")
                .unwrap_or("0x2036807B0B3aaf5b1858EE822D0e111fDdac7018".to_string())
        )
        .unwrap(),
        glm_multi_transfer_contract_address: None,
        glm_faucet_address: None,
        required_confirmations: {
            match env::var("ERC20_MUMBAI_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        },
        payment_max_processed: {
            match env::var("ERC20_MUMBAI_PAYMENT_MAX_PROCESSED").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => DEFAULT_MAX_PROCESSED,
            }
        },
        latest_block_max_seconds_behind: {
            match env::var("ERC20_MUMBAI_LATEST_BLOCK_SECONDS_BEHIND").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 600,
            }
        },
        use_external_gas_provider: false
    };
    pub static ref POLYGON_MAINNET_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("POLYGON_GLM_CONTRACT_ADDRESS")
                .unwrap_or("0x0b220b82f3ea3b7f6d9a1d8ab58930c064a2b5bf".to_string())
        )
        .unwrap(),
        glm_multi_transfer_contract_address: Some(utils::str_to_addr(
            &env::var("POLYGON_MULTI_TRANSFER_CONTRACT_ADDRESS")
                .unwrap_or("0x21cCe3a0F851394fcDD27b28c65232be98fc6Ce2".to_string())
        )
        .unwrap()),
        glm_faucet_address: None,
        required_confirmations: {
            match env::var("ERC20_POLYGON_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 5,
            }
        },
        payment_max_processed: {
            match env::var("ERC20_POLYGON_PAYMENT_MAX_PROCESSED").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => DEFAULT_MAX_PROCESSED,
            }
        },
        latest_block_max_seconds_behind: {
            match env::var("ERC20_POLYGON_LATEST_BLOCK_SECONDS_BEHIND").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 30,
            }
        },
        use_external_gas_provider: false
    };
}
