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
    pub glm_faucet_address: Option<Address>,
    pub required_confirmations: u64,
}

lazy_static! {
    pub static ref MAINNET_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("MAINNET_GLM_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0x7DD9c5Cba05E151C895FDe1CF355C9A1D5DA6429".to_string())
        )
        .unwrap(),
        glm_faucet_address: None,
        required_confirmations: {
            match env::var("ERC20_MAINNET_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 5,
            }
        }
    };
    pub static ref RINKEBY_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("RINKEBY_TGLM_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0xd94e3DC39d4Cad1DAd634e7eb585A57A19dC7EFE".to_string())
        )
        .unwrap(),
        glm_faucet_address: Some(
            utils::str_to_addr(
                &env::var("RINKEBY_TGLM_FAUCET_ADDRESS")
                    .unwrap_or_else(|_| "0x59259943616265A03d775145a2eC371732E2B06C".to_string())
            )
            .unwrap()
        ),
        required_confirmations: {
            match env::var("ERC20_RINKEBY_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        }
    };
    pub static ref GOERLI_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("GOERLI_TGLM_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0x33af15c79d64b85ba14aaffaa4577949104b22e8".to_string())
        )
        .unwrap(),
        glm_faucet_address: Some(
            utils::str_to_addr(
                &env::var("GOERLI_TGLM_FAUCET_ADDRESS")
                    .unwrap_or_else(|_| "0xCCA41b09C1F50320bFB41BD6822BD0cdBDC7d85C".to_string())
            )
            .unwrap()
        ),
        required_confirmations: {
            match env::var("ERC20_GOERLI_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        }
    };
    pub static ref SEPOLIA_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("SEPOLIA_TGLM_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0x167b15ada84c63427c6c813B915a42eFC72E7175".to_string())
        )
        .unwrap(),
        glm_faucet_address: Some(
            utils::str_to_addr(
                &env::var("SEPOLIA_TGLM_FAUCET_ADDRESS")
                    .unwrap_or_else(|_| "0x31A2a20956a40c2F358Fa5cec59D55a9C5d6fF9A".to_string())
            )
            .unwrap()
        ),
        required_confirmations: {
            match env::var("ERC20_SEPOLIA_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        }
    };
    pub static ref HOLESKY_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("HOLESKY_TGLM_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0x8888888815bf4DB87e57B609A50f938311EEd068".to_string())
        )
        .unwrap(),
        glm_faucet_address: Some(
            utils::str_to_addr(
                &env::var("HOLESKY_TGLM_FAUCET_ADDRESS")
                    .unwrap_or_else(|_| "0xFACe100969FF47EB58d2CF603321B581A84bcEaC".to_string())
            )
            .unwrap()
        ),
        required_confirmations: {
            match env::var("ERC20_HOLESKY_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        }
    };
    pub static ref HOODI_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("HOODI_TGLM_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0x55555555555556AcFf9C332Ed151758858bd7a26".to_string())
        )
        .unwrap(),
        glm_faucet_address: Some(
            utils::str_to_addr(
                &env::var("HOODI_TGLM_FAUCET_ADDRESS")
                    .unwrap_or_else(|_| "0x500F965199C63865A3E666cA3fF55B64F1c8Bc8b".to_string())
            )
            .unwrap()
        ),
        required_confirmations: {
            match env::var("ERC20_HOODI_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        }
    };
    pub static ref MUMBAI_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("MUMBAI_TGLM_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0x2036807B0B3aaf5b1858EE822D0e111fDdac7018".to_string())
        )
        .unwrap(),
        glm_faucet_address: None,
        required_confirmations: {
            match env::var("ERC20_MUMBAI_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        }
    };
    pub static ref AMOY_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("AMOY_TGLM_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0x2b60e60d3fb0b36a7ccb388f9e71570da4c4594f".to_string())
        )
        .unwrap(),
        glm_faucet_address: Some(
            utils::str_to_addr(
                &env::var("AMOY_TGLM_FAUCET_ADDRESS")
                    .unwrap_or_else(|_| "0xf29ff8a13211ac33861986e407190ae5c773d53c".to_string())
            )
            .unwrap()
        ),
        required_confirmations: {
            match env::var("ERC20_AMOY_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 3,
            }
        }
    };
    pub static ref POLYGON_MAINNET_CONFIG: EnvConfiguration = EnvConfiguration {
        glm_contract_address: utils::str_to_addr(
            &env::var("POLYGON_GLM_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0x0b220b82f3ea3b7f6d9a1d8ab58930c064a2b5bf".to_string())
        )
        .unwrap(),
        glm_faucet_address: None,
        required_confirmations: {
            match env::var("ERC20_POLYGON_REQUIRED_CONFIRMATIONS").map(|s| s.parse()) {
                Ok(Ok(x)) => x,
                _ => 5,
            }
        }
    };
}
