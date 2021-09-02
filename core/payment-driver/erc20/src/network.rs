use maplit::hashmap;
use std::collections::HashMap;
use std::str::FromStr;

// Workspace uses
use ya_payment_driver::{db::models::Network as DbNetwork, driver::Network, model::GenericError};

// Local uses
use crate::{
    RINKEBY_NETWORK, RINKEBY_PLATFORM, RINKEBY_TOKEN, GOERLI_NETWORK, GOERLI_PLATFORM, GOERLI_TOKEN, MAINNET_NETWORK, MAINNET_PLATFORM,
    MAINNET_TOKEN,
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
        MAINNET_NETWORK.to_string() => Network {
            default_token: MAINNET_TOKEN.to_string(),
            tokens: hashmap! {
                MAINNET_TOKEN.to_string() => MAINNET_PLATFORM.to_string()
            }
        }
    };
    pub static ref RINKEBY_DB_NETWORK: DbNetwork = DbNetwork::from_str(RINKEBY_NETWORK).unwrap();
    pub static ref GOERLI_DB_NETWORK: DbNetwork = DbNetwork::from_str(GOERLI_NETWORK).unwrap();
    pub static ref MAINNET_DB_NETWORK: DbNetwork = DbNetwork::from_str(MAINNET_NETWORK).unwrap();
}

pub fn platform_to_network_token(platform: String) -> Result<(DbNetwork, String), GenericError> {
    match platform.as_str() {
        RINKEBY_PLATFORM => Ok((*RINKEBY_DB_NETWORK, RINKEBY_TOKEN.to_owned())),
        GOERLI_PLATFORM => Ok((*GOERLI_DB_NETWORK, GOERLI_TOKEN.to_owned())),
        MAINNET_PLATFORM => Ok((*MAINNET_DB_NETWORK, MAINNET_TOKEN.to_owned())),
        other => Err(GenericError::new(format!(
            "Unable to find network for platform: {}",
            other
        ))),
    }
}

pub fn network_token_to_platform(
    network: Option<DbNetwork>,
    token: Option<String>,
) -> Result<String, GenericError> {
    let network = network.unwrap_or(*RINKEBY_DB_NETWORK);
    let network_config = (*SUPPORTED_NETWORKS).get(&(network.to_string()));
    let network_config = match network_config {
        Some(nc) => nc,
        None => {
            return Err(GenericError::new(format!(
                "Unable to find platform for network={}",
                network
            )))
        }
    };

    let token = token.unwrap_or(network_config.default_token.clone());
    let platform = network_config.tokens.get(&token);
    let platform = match platform {
        Some(p) => p,
        None => {
            return Err(GenericError::new(format!(
                "Unable to find platform for token={}",
                token
            )))
        }
    };
    Ok(platform.to_string())
}

pub fn get_network_token(network: DbNetwork, token: Option<String>) -> String {
    // Fetch network config, safe as long as all DbNetwork entries are in SUPPORTED_NETWORKS
    let network_config = (*SUPPORTED_NETWORKS).get(&(network.to_string())).unwrap();
    // TODO: Check if token in network.tokens
    token.unwrap_or(network_config.default_token.clone())
}

pub fn network_like_to_network(network_like: Option<String>) -> DbNetwork {
    match network_like {
        Some(n) => DbNetwork::from_str(&n).unwrap_or(*RINKEBY_DB_NETWORK),
        None => *RINKEBY_DB_NETWORK,
    }
}
