use crate::utils;
use crate::{GNTDriverError, GNTDriverResult};
use ethereum_types::Address;
use std::{env, time};

const MAX_ETH_FAUCET_REQUESTS: u32 = 6;
const ETH_FAUCET_SLEEP: time::Duration = time::Duration::from_secs(2);
const INIT_ETH_SLEEP: time::Duration = time::Duration::from_secs(15);
const ETH_FAUCET_ADDRESS_ENV_VAR: &str = "ETH_FAUCET_ADDRESS";

pub struct EthFaucetConfig {
    faucet_address: awc::http::Uri,
}

impl EthFaucetConfig {
    pub fn from_env() -> GNTDriverResult<Self> {
        let faucet_address_str = env::var(ETH_FAUCET_ADDRESS_ENV_VAR)
            .map_err(|_| GNTDriverError::MissingEnvironmentVariable(ETH_FAUCET_ADDRESS_ENV_VAR))?;
        let faucet_address = faucet_address_str
            .parse()
            .map_err(|e| GNTDriverError::LibraryError(format!("invalid faucet address: {}", e)))?;
        Ok(EthFaucetConfig { faucet_address })
    }

    pub async fn request_eth(&self, address: Address) -> GNTDriverResult<()> {
        log::debug!("request eth");
        let client = awc::Client::new();
        let request_url = format!("{}/{}", &self.faucet_address, utils::addr_to_str(address));

        async fn try_request_eth(client: &awc::Client, url: &str) -> GNTDriverResult<()> {
            let body = client
                .get(url)
                .send()
                .await
                .map_err(|e| GNTDriverError::LibraryError(e.to_string()))?
                .body()
                .await
                .map_err(|e| GNTDriverError::LibraryError(e.to_string()))?;
            let resp = std::string::String::from_utf8_lossy(body.as_ref());
            if resp.contains("sufficient funds") || resp.contains("txhash") {
                log::debug!("resp: {}", resp);
                return Ok(());
            }

            Err(GNTDriverError::LibraryError(resp.into_owned()))
        }

        for _ in 0..MAX_ETH_FAUCET_REQUESTS {
            if let Err(e) = try_request_eth(&client, &request_url).await {
                log::error!("Failed to request Eth from Faucet: {:?}", e);
                tokio::time::delay_for(ETH_FAUCET_SLEEP).await;
            } else {
                tokio::time::delay_for(INIT_ETH_SLEEP).await;
                return Ok(());
            }
        }
        Err(GNTDriverError::LibraryError(format!(
            "Cannot request Eth from Faucet"
        )))
    }
}
