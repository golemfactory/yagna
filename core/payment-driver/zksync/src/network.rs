use maplit::hashmap;
use std::collections::HashMap;
use std::str::FromStr;

// Workspace uses
use ya_payment_driver::{db::models::Network as DbNetwork, driver::Network, model::GenericError};

// Local uses
use crate::{
    DEFAULT_NETWORK, DEFAULT_PLATFORM, DEFAULT_TOKEN, MAINNET_NETWORK, MAINNET_PLATFORM,
    MAINNET_TOKEN,
};

lazy_static::lazy_static! {
    pub static ref SUPPORTED_NETWORKS: HashMap<String, Network> = hashmap! {
        DEFAULT_NETWORK.to_string() => Network {
            default_token: DEFAULT_TOKEN.to_string(),
            tokens: hashmap! {
                DEFAULT_TOKEN.to_string() => DEFAULT_PLATFORM.to_string()
            }
        },
        MAINNET_NETWORK.to_string() => Network {
            default_token: MAINNET_TOKEN.to_string(),
            tokens: hashmap! {
                MAINNET_TOKEN.to_string() => MAINNET_PLATFORM.to_string()
            }
        }
    };
    static ref DEFAULT_DB_NETWORK: DbNetwork = DbNetwork::from_str(DEFAULT_NETWORK).unwrap();
    static ref MAINNET_DB_NETWORK: DbNetwork = DbNetwork::from_str(MAINNET_NETWORK).unwrap();
}

pub fn platform_to_network_token(platform: String) -> Result<(DbNetwork, String), GenericError> {
    match platform.as_str() {
        DEFAULT_PLATFORM => Ok((*DEFAULT_DB_NETWORK, DEFAULT_TOKEN.to_owned())),
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
    let network =
        network.unwrap_or(DbNetwork::from_str(DEFAULT_NETWORK).map_err(GenericError::new)?);
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

    let token = token.unwrap_or_else(|| network_config.default_token.clone());
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
    token.unwrap_or_else(|| network_config.default_token.clone())
}
