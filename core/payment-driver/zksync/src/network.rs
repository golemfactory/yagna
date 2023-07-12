use maplit::hashmap;
use std::collections::HashMap;
use std::str::FromStr;

// Workspace uses
use ya_payment_driver::{db::models::Network as DbNetwork, driver::Network, model::GenericError};

// Local uses
use crate::{DEFAULT_NETWORK, MAINNET_NETWORK, MAINNET_PLATFORM, MAINNET_TOKEN};

lazy_static::lazy_static! {
    pub static ref SUPPORTED_NETWORKS: HashMap<String, Network> = hashmap! {
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
        MAINNET_PLATFORM => Ok((*MAINNET_DB_NETWORK, MAINNET_TOKEN.to_owned())),
        other => Err(GenericError::new(format!(
            "Unable to find network for platform: {}",
            other
        ))),
    }
}

pub fn network_token_to_platform(
    network: DbNetwork,
    token: Option<String>,
) -> Result<String, GenericError> {
    let network_config = get_network_config(&network)?;
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

pub fn get_network_token(
    network: DbNetwork,
    token: Option<String>,
) -> Result<String, GenericError> {
    let network_config = get_network_config(&network)?;
    Ok(token.unwrap_or_else(|| network_config.default_token.clone()))
}

pub fn get_network_config(network: &DbNetwork) -> Result<&Network, GenericError> {
    let network_config = (*SUPPORTED_NETWORKS).get(&(network.to_string()));
    match network_config {
        Some(network_config) => Ok(network_config),
        None => Err(GenericError::new(format!(
            "Network {} is not supported",
            network
        ))),
    }
}
