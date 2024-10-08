use maplit::hashmap;
use std::collections::HashMap;
use std::str::FromStr;

// Workspace uses
use ya_payment_driver::{db::models::Network as DbNetwork, driver::Network, model::GenericError};

// Local uses
use crate::{
    AMOY_CURRENCY_LONG, AMOY_CURRENCY_SHORT, AMOY_NETWORK, AMOY_PLATFORM, AMOY_TOKEN,
    GOERLI_CURRENCY_LONG, GOERLI_CURRENCY_SHORT, GOERLI_NETWORK, GOERLI_PLATFORM, GOERLI_TOKEN,
    HOLESKY_CURRENCY_LONG, HOLESKY_CURRENCY_SHORT, HOLESKY_NETWORK, HOLESKY_PLATFORM,
    HOLESKY_TOKEN, MAINNET_CURRENCY_LONG, MAINNET_CURRENCY_SHORT, MAINNET_NETWORK,
    MAINNET_PLATFORM, MAINNET_TOKEN, MUMBAI_CURRENCY_LONG, MUMBAI_CURRENCY_SHORT, MUMBAI_NETWORK,
    MUMBAI_PLATFORM, MUMBAI_TOKEN, POLYGON_MAINNET_CURRENCY_LONG, POLYGON_MAINNET_CURRENCY_SHORT,
    POLYGON_MAINNET_NETWORK, POLYGON_MAINNET_PLATFORM, POLYGON_MAINNET_TOKEN,
    RINKEBY_CURRENCY_LONG, RINKEBY_CURRENCY_SHORT, RINKEBY_NETWORK, RINKEBY_PLATFORM,
    RINKEBY_TOKEN, SEPOLIA_CURRENCY_LONG, SEPOLIA_CURRENCY_SHORT, SEPOLIA_NETWORK,
    SEPOLIA_PLATFORM, SEPOLIA_TOKEN,
};

lazy_static::lazy_static! {
    pub static ref SUPPORTED_NETWORKS: HashMap<String, Network> = hashmap! {
        RINKEBY_NETWORK.to_string() => Network {
            default_token: RINKEBY_TOKEN.to_string(),
            tokens: hashmap! {
                RINKEBY_TOKEN.to_string() => RINKEBY_PLATFORM.to_string()
            }
        },
        GOERLI_NETWORK.to_string() => Network {
            default_token: GOERLI_TOKEN.to_string(),
            tokens: hashmap! {
                GOERLI_TOKEN.to_string() => GOERLI_PLATFORM.to_string()
            }
        },
        HOLESKY_NETWORK.to_string() => Network {
            default_token: HOLESKY_TOKEN.to_string(),
            tokens: hashmap! {
                HOLESKY_TOKEN.to_string() => HOLESKY_PLATFORM.to_string()
            }
        },
        SEPOLIA_NETWORK.to_string() => Network {
            default_token: SEPOLIA_TOKEN.to_string(),
            tokens: hashmap! {SEPOLIA_TOKEN.to_string() => SEPOLIA_PLATFORM.to_string()}
        },
        MAINNET_NETWORK.to_string() => Network {
            default_token: MAINNET_TOKEN.to_string(),
            tokens: hashmap! {
                MAINNET_TOKEN.to_string() => MAINNET_PLATFORM.to_string()
            }
        },
        MUMBAI_NETWORK.to_string() => Network {
            default_token: MUMBAI_TOKEN.to_string(),
            tokens: hashmap! {
                MUMBAI_TOKEN.to_string() => MUMBAI_PLATFORM.to_string()
            }
        },
        AMOY_NETWORK.to_string() => Network {
            default_token: AMOY_TOKEN.to_string(),
            tokens: hashmap! {
                AMOY_TOKEN.to_string() => AMOY_PLATFORM.to_string()
            }
        },
        POLYGON_MAINNET_NETWORK.to_string() => Network {
            default_token: POLYGON_MAINNET_TOKEN.to_string(),
            tokens: hashmap! {
                POLYGON_MAINNET_TOKEN.to_string() => POLYGON_MAINNET_PLATFORM.to_string()
            }
        }
    };
    pub static ref RINKEBY_DB_NETWORK: DbNetwork = DbNetwork::from_str(RINKEBY_NETWORK).unwrap();
    pub static ref GOERLI_DB_NETWORK: DbNetwork = DbNetwork::from_str(GOERLI_NETWORK).unwrap();
    pub static ref HOLESKY_DB_NETWORK: DbNetwork = DbNetwork::from_str(HOLESKY_NETWORK).unwrap();
    pub static ref MAINNET_DB_NETWORK: DbNetwork = DbNetwork::from_str(MAINNET_NETWORK).unwrap();
    pub static ref MUMBAI_DB_NETWORK: DbNetwork = DbNetwork::from_str(MUMBAI_NETWORK).unwrap();
    pub static ref AMOY_DB_NETWORK: DbNetwork = DbNetwork::from_str(AMOY_NETWORK).unwrap();
    pub static ref POLYGON_MAINNET_DB_NETWORK: DbNetwork = DbNetwork::from_str(POLYGON_MAINNET_NETWORK).unwrap();
}

pub fn platform_to_network_token(platform: String) -> Result<(DbNetwork, String), GenericError> {
    match platform.as_str() {
        RINKEBY_PLATFORM => Ok((*RINKEBY_DB_NETWORK, RINKEBY_TOKEN.to_owned())),
        GOERLI_PLATFORM => Ok((*GOERLI_DB_NETWORK, GOERLI_TOKEN.to_owned())),
        HOLESKY_PLATFORM => Ok((*HOLESKY_DB_NETWORK, HOLESKY_TOKEN.to_owned())),
        MAINNET_PLATFORM => Ok((*MAINNET_DB_NETWORK, MAINNET_TOKEN.to_owned())),
        MUMBAI_PLATFORM => Ok((*MUMBAI_DB_NETWORK, MUMBAI_TOKEN.to_owned())),
        AMOY_PLATFORM => Ok((*AMOY_DB_NETWORK, AMOY_TOKEN.to_owned())),
        POLYGON_MAINNET_PLATFORM => Ok((
            *POLYGON_MAINNET_DB_NETWORK,
            POLYGON_MAINNET_TOKEN.to_owned(),
        )),
        other => Err(GenericError::new(format!(
            "Unable to find network for platform: {}",
            other
        ))),
    }
}

pub fn platform_to_currency(platform: String) -> Result<(String, String), GenericError> {
    match platform.as_str() {
        RINKEBY_PLATFORM => Ok((
            RINKEBY_CURRENCY_SHORT.to_owned(),
            RINKEBY_CURRENCY_LONG.to_owned(),
        )),
        GOERLI_PLATFORM => Ok((
            GOERLI_CURRENCY_SHORT.to_owned(),
            GOERLI_CURRENCY_LONG.to_owned(),
        )),
        HOLESKY_PLATFORM => Ok((
            HOLESKY_CURRENCY_SHORT.to_owned(),
            HOLESKY_CURRENCY_LONG.to_owned(),
        )),
        SEPOLIA_PLATFORM => Ok((
            SEPOLIA_CURRENCY_SHORT.to_owned(),
            SEPOLIA_CURRENCY_LONG.to_string(),
        )),
        MAINNET_PLATFORM => Ok((
            MAINNET_CURRENCY_SHORT.to_owned(),
            MAINNET_CURRENCY_LONG.to_owned(),
        )),
        MUMBAI_PLATFORM => Ok((
            MUMBAI_CURRENCY_SHORT.to_owned(),
            MUMBAI_CURRENCY_LONG.to_owned(),
        )),
        AMOY_PLATFORM => Ok((
            AMOY_CURRENCY_SHORT.to_owned(),
            AMOY_CURRENCY_LONG.to_owned(),
        )),
        POLYGON_MAINNET_PLATFORM => Ok((
            POLYGON_MAINNET_CURRENCY_SHORT.to_owned(),
            POLYGON_MAINNET_CURRENCY_LONG.to_owned(),
        )),
        other => Err(GenericError::new(format!(
            "Unable to find network currency for platform: {}",
            other
        ))),
    }
}

pub fn get_network_token(network: DbNetwork, token: Option<String>) -> String {
    // Fetch network config, safe as long as all DbNetwork entries are in SUPPORTED_NETWORKS
    let network_config = (*SUPPORTED_NETWORKS).get(&(network.to_string())).unwrap();
    // TODO: Check if token in network.tokens
    token.unwrap_or_else(|| network_config.default_token.clone())
}

pub fn network_like_to_network(network_like: Option<String>) -> DbNetwork {
    match network_like {
        Some(n) => DbNetwork::from_str(&n).unwrap_or(*HOLESKY_DB_NETWORK),
        None => *HOLESKY_DB_NETWORK,
    }
}
