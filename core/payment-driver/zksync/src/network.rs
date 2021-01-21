use maplit::hashmap;
use std::collections::HashMap;
use std::str::FromStr;

// Workspace uses
use ya_payment_driver::{db::network::Network as DbNetwork, model::GenericError, driver::Network};

// Local uses
use crate::{
    DEFAULT_NETWORK, DEFAULT_PLATFORM, DEFAULT_TOKEN, MAINNET_NETWORK, MAINNET_PLATFORM, MAINNET_TOKEN,
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
}

pub fn platform_to_network_token(platform: String) -> Result<(DbNetwork, String), GenericError> {
    let net_list = (*SUPPORTED_NETWORKS).clone();
    for network in net_list {
        for token in network.1.tokens {
            if platform == token.1 {
                let db_network = DbNetwork::from_str(&network.0).map_err(GenericError::new)?;
                return Ok((db_network, token.0));
            }
        }
    };
    Err(GenericError::new(format!("Unable to find network for platform: {}", platform)))
}
