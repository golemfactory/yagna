use std::{convert::TryInto, fmt::Display, str::FromStr};

use super::{
    token_name::TokenName, DEFAULT_MAINNET_NETWORK, DEFAULT_PAYMENT_DRIVER, DEFAULT_TESTNET_NETWORK,
};
use anyhow::{anyhow, bail};
use ya_client_model::payment::allocation::PaymentPlatform;
use ya_core_model::payment::local::{DriverName, NetworkName};

pub struct PaymentPlatformTriple {
    driver: DriverName,
    network: NetworkName,
    token: TokenName,
}

impl PaymentPlatformTriple {
    pub fn driver(&self) -> &DriverName {
        &self.driver
    }

    pub fn network(&self) -> &NetworkName {
        &self.network
    }

    pub fn token(&self) -> &TokenName {
        &self.token
    }

    pub fn default_testnet() -> Self {
        PaymentPlatformTriple {
            driver: DEFAULT_PAYMENT_DRIVER,
            network: DEFAULT_TESTNET_NETWORK,
            token: TokenName::default(&DEFAULT_PAYMENT_DRIVER, &DEFAULT_TESTNET_NETWORK),
        }
    }

    pub fn default_mainnet() -> Self {
        PaymentPlatformTriple {
            driver: DEFAULT_PAYMENT_DRIVER,
            network: DEFAULT_MAINNET_NETWORK,
            token: TokenName::default(&DEFAULT_PAYMENT_DRIVER, &DEFAULT_MAINNET_NETWORK),
        }
    }

    pub fn from_payment_platform_input(
        p: &PaymentPlatform,
    ) -> anyhow::Result<PaymentPlatformTriple> {
        let platform = if p.driver.is_none() && p.network.is_none() && p.token.is_none() {
            let default_platform = Self::default_testnet();
            log::debug!("Empty paymentPlatform object, using {default_platform}");
            default_platform
        } else if p.token.is_some() && p.network.is_none() && p.driver.is_none() {
            let token = p.token.as_ref().unwrap();
            if token == "GLM" || token == "tGLM" {
                bail!(
                        "Uppercase token names are not supported. Use lowercase glm or tglm instead of {}",
                        token
                    );
            } else if token == "glm" {
                let default_platform = Self::default_mainnet();
                log::debug!("Selected network {default_platform} (default for glm token)");
                default_platform
            } else if token == "tglm" {
                let default_platform = Self::default_testnet();
                log::debug!("Selected network {default_platform} (default for tglm token)");
                default_platform
            } else {
                bail!("Only glm or tglm token values are accepted vs {token} provided");
            }
        } else {
            let network_str = p.network.as_deref().unwrap_or_else(|| {
                if let Some(token) = p.token.as_ref() {
                    if token == "glm" {
                        log::debug!(
                            "Network not specified, using default {}, because token set to glm",
                            DEFAULT_MAINNET_NETWORK
                        );
                        DEFAULT_MAINNET_NETWORK.into()
                    } else {
                        log::debug!(
                            "Network not specified, using default {}",
                            DEFAULT_TESTNET_NETWORK
                        );
                        DEFAULT_TESTNET_NETWORK.into()
                    }
                } else {
                    log::debug!(
                        "Network not specified and token not specified, using default {}",
                        DEFAULT_TESTNET_NETWORK
                    );
                    DEFAULT_TESTNET_NETWORK.into()
                }
            });
            let network = validate_network(network_str)
                .map_err(|err| anyhow!("Validate network failed (1): {err}"))?;

            let driver_str = p.driver.as_deref().unwrap_or_else(|| {
                log::debug!(
                    "Driver not specified, using default {}",
                    DEFAULT_PAYMENT_DRIVER
                );
                DEFAULT_PAYMENT_DRIVER.into()
            });
            let driver = validate_driver(&network, driver_str)
                .map_err(|err| anyhow!("Validate driver failed (1): {err}"))?;

            if let Some(token) = p.token.as_ref() {
                let token = TokenName::from_token_string(&driver, &network, token)
                    .map_err(|err| anyhow!("Validate token failed (1): {err}"))?;
                log::debug!("Selected network {}-{}-{}", driver, network, token);
                Self {
                    driver,
                    network,
                    token,
                }
            } else {
                let default_token = TokenName::default(&driver, &network);

                log::debug!(
                    "Selected network with default token {}-{}-{}",
                    driver,
                    network,
                    default_token
                );
                Self {
                    driver,
                    network,
                    token: default_token,
                }
            }
        };
        Ok(platform)
    }

    pub fn from_payment_platform_str(
        payment_platform_str: &str,
    ) -> anyhow::Result<PaymentPlatformTriple> {
        // payment_platform is of the form driver-network-token
        // eg. erc20-rinkeby-tglm
        let [driver_str, network_str, token_str]: [&str; 3] = payment_platform_str
            .split('-')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|err| {
                anyhow!(
                    "paymentPlatform must be of the form driver-network-token instead of {}",
                    payment_platform_str
                )
            })?;

        let network = validate_network(network_str)
            .map_err(|err| anyhow!("Validate network failed (2): {err}"))?;

        let driver = validate_driver(&network, driver_str)
            .map_err(|err| anyhow!("Validate driver failed (2): {err}"))?;

        let token = TokenName::from_token_string(&driver, &network, token_str)
            .map_err(|err| anyhow!("Validate token failed (2): {err}"))?;

        Ok(Self {
            driver,
            network,
            token,
        })
    }
}

impl Display for PaymentPlatformTriple {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}-{}", self.driver, self.network, self.token)
    }
}

fn validate_network(network: &str) -> Result<NetworkName, String> {
    match NetworkName::from_str(network) {
        Ok(NetworkName::Rinkeby) => Err("Rinkeby is no longer supported".to_string()),
        Ok(network_name) => Ok(network_name),
        Err(_) => Err(format!("Invalid network name: {network}")),
    }
}

fn validate_driver(network: &NetworkName, driver: &str) -> Result<DriverName, String> {
    match DriverName::from_str(driver) {
        Err(_) => Err(format!("Invalid driver name {}", driver)),
        Ok(driver_name) => Ok(driver_name),
    }
}
