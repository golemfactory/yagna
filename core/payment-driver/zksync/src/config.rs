// External
use maplit::hashmap;
use std::collections::HashMap;
use std::str::FromStr;

// Workspace
use ya_payment_driver::db::models::Network as DbNetwork;
use ya_payment_driver::driver::Network;
use ya_payment_driver::model::GenericError;
use ya_utils_networking::srv_resolver;

#[derive(Debug, Clone)]
pub struct DriverConfig {
    pub name: String,
    pub default_network: DbNetwork,
    pub networks: HashMap<DbNetwork, NetworkConfig>,
}

impl DriverConfig {
    pub fn resolve_network(&self, network: Option<&str>) -> Result<DbNetwork, GenericError> {
        match network {
            Some(network) => DbNetwork::from_str(network).map_err(GenericError::new),
            None => Ok(self.default_network),
        }
    }

    pub fn supported_networks(&self) -> HashMap<String, Network> {
        self.networks
            .iter()
            .map(|(network, net_config)| {
                let tokens = hashmap! {
                    net_config.token.clone() => net_config.platform.clone()
                };
                (
                    network.to_string(),
                    Network {
                        default_token: net_config.token.clone(),
                        tokens,
                    },
                )
            })
            .collect()
    }

    pub fn platform_to_network_token(
        &self,
        platform: &str,
    ) -> Result<(DbNetwork, String), GenericError> {
        for (network, net_config) in self.networks.iter() {
            if net_config.platform == platform {
                return Ok((network.to_owned(), net_config.token.clone()));
            }
        }
        Err(GenericError::new(format!(
            "Unable to find network for platform: {}",
            platform
        )))
    }

    pub fn network_token_to_platform(
        &self,
        network: Option<DbNetwork>,
        token: Option<&str>,
    ) -> Result<String, GenericError> {
        let network = network.unwrap_or(self.default_network);
        let net_config = match self.networks.get(&network) {
            Some(config) => config,
            None => {
                return Err(GenericError::new(format!(
                    "Unable to find platform for network={}",
                    network
                )))
            }
        };

        match token {
            Some(token) if token != &net_config.token => Err(GenericError::new(format!(
                "Unable to find platform for token={}",
                token
            ))),
            _ => Ok(net_config.platform.clone()),
        }
    }

    pub fn get_network_token(&self, network: DbNetwork, token: Option<String>) -> String {
        // TODO: Check if token is supported
        token.unwrap_or_else(|| self.networks.get(&network).unwrap().token.clone())
    }
}

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub token: String,        // Token name used in yagna
    pub token_zksync: String, // Token name used in zkSync API
    pub platform: String,
    pub rpc_addr_env_var: String,
    pub default_rpc_addr: Option<String>,
    pub faucet: Option<FaucetConfig>,
}

impl NetworkConfig {
    pub fn rpc_addr(&self) -> Option<String> {
        std::env::var(&self.rpc_addr_env_var)
            .ok()
            .or_else(|| self.default_rpc_addr.clone())
    }

    pub async fn resolve_faucet_url(&self) -> Option<String> {
        match &self.faucet {
            None => None,
            Some(faucet) => match faucet.resolve_faucet_url().await {
                Ok(url) => Some(url),
                Err(e) => {
                    log::error!("Error resolving faucet URL: {}", e);
                    None
                }
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct FaucetConfig {
    pub addr_env_var: String,
    pub srv_prefix: String,
    pub srv_postfix: String,
}

impl FaucetConfig {
    pub async fn resolve_faucet_url(&self) -> Result<String, GenericError> {
        match std::env::var(&self.addr_env_var) {
            Ok(addr) => Ok(addr),
            _ => {
                let faucet_host = srv_resolver::resolve_yagna_record(&self.srv_prefix)
                    .await
                    .map_err(|_| GenericError::new("Faucet SRV record cannot be resolved"))?;

                Ok(format!("http://{}{}", faucet_host, &self.srv_postfix))
            }
        }
    }
}

lazy_static! {
    pub static ref ZKSYNC_CONFIG: DriverConfig = DriverConfig {
        name: "zksync".to_string(),
        default_network: DbNetwork::Rinkeby,
        networks: hashmap! {
            DbNetwork::Rinkeby => NetworkConfig {
                token: "tGLM".to_string(),
                token_zksync: "GNT".to_string(),
                platform: "zksync-rinkeby-tglm".to_string(),
                rpc_addr_env_var: "ZKSYNC_RINKEBY_RPC_ADDRESS".to_string(),
                default_rpc_addr: None,
                faucet: Some(FaucetConfig {
                    addr_env_var: "ZKSYNC_FAUCET_ADDR".to_string(),
                    srv_prefix: "_zk-faucet._tcp".to_string(),
                    srv_postfix: "/zk/donatex".to_string()
                }),
            },
            DbNetwork::Mainnet => NetworkConfig {
                token: "GLM".to_string(),
                token_zksync: "GLM".to_string(),
                platform: "zksync-mainnet-glm".to_string(),
                rpc_addr_env_var: "ZKSYNC_MAINNET_RPC_ADDRESS".to_string(),
                default_rpc_addr: None,
                faucet: None,
            }
        },
    };

    pub static ref GLMSYNC_CONFIG: DriverConfig = DriverConfig {
        name: "glmsync".to_string(),
        default_network: DbNetwork::Rinkeby,
        networks: hashmap! {
            DbNetwork::Rinkeby => NetworkConfig {
                token: "tGLM".to_string(),
                token_zksync: "GLM".to_string(),
                platform: "glmsync-rinkeby-tglm".to_string(),
                rpc_addr_env_var: "GLMSYNC_RINKEBY_RPC_ADDRESS".to_string(),
                default_rpc_addr: Some("http://rinkeby-api.zksync.imapp.pl/jsrpc".to_string()),
                faucet: Some(FaucetConfig {
                    addr_env_var: "GLMSYNC_FAUCET_ADDR".to_string(),
                    srv_prefix: "FIXME".to_string(), // FIXME
                    srv_postfix: "/zk/donatex".to_string()
                }),
            },
            DbNetwork::Mainnet => NetworkConfig {
                token: "GLM".to_string(),
                token_zksync: "GLM".to_string(),
                platform: "glmsync-mainnet-glm".to_string(),
                rpc_addr_env_var: "FIXME".to_string(), // FIXME
                default_rpc_addr: Some("FIXME".to_string()), // FIXME,
                faucet: None,
            }
        },
    };
}
