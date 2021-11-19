use maplit::hashmap;
use std::collections::HashMap;
use std::str::FromStr;

// Workspace uses
use ya_payment_driver::{db::models::Network as DbNetwork, driver::Network, model::GenericError};

// Local uses
use crate::{
    MUMBAI_NETWORK, MUMBAI_PLATFORM, MUMBAI_TOKEN, POLYGON_MAINNET_NETWORK,
    POLYGON_MAINNET_PLATFORM, POLYGON_MAINNET_TOKEN,
};

lazy_static::lazy_static! {
    pub static ref SUPPORTED_NETWORKS: HashMap<String, Network> = hashmap! {
        MUMBAI_NETWORK.to_string() => Network {
            default_token: MUMBAI_TOKEN.to_string(),
            tokens: hashmap! {
                MUMBAI_TOKEN.to_string() => MUMBAI_PLATFORM.to_string()
            }
        },
        POLYGON_MAINNET_NETWORK.to_string() => Network {
            default_token: POLYGON_MAINNET_TOKEN.to_string(),
            tokens: hashmap! {
                POLYGON_MAINNET_TOKEN.to_string() => POLYGON_MAINNET_PLATFORM.to_string()
            }
        }
    };
    pub static ref MUMBAI_DB_NETWORK: DbNetwork = DbNetwork::from_str(MUMBAI_NETWORK).unwrap();
    pub static ref POLYGON_MAINNET_DB_NETWORK: DbNetwork = DbNetwork::from_str(POLYGON_MAINNET_NETWORK).unwrap();
}

pub fn platform_to_network_token(platform: String) -> Result<(DbNetwork, String), GenericError> {
    match platform.as_str() {
        MUMBAI_PLATFORM => Ok((*MUMBAI_DB_NETWORK, MUMBAI_TOKEN.to_owned())),
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

pub fn network_token_to_platform(
    network: Option<DbNetwork>,
    token: Option<String>,
) -> Result<String, GenericError> {
    let network = network.unwrap_or(*MUMBAI_DB_NETWORK);
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
        Some(n) => DbNetwork::from_str(&n).unwrap_or(*MUMBAI_DB_NETWORK),
        None => *MUMBAI_DB_NETWORK,
    }
}
