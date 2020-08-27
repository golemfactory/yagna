use crate::utils;
use crate::{PaymentDriverError, PaymentDriverResult};
use ethereum_types::Address;
use hyper::body::HttpBody as _;
use hyper::client::HttpConnector;
use hyper::http::uri::InvalidUri;
use hyper::{Body, Uri};
use std::{env, time};

const MAX_ETH_FAUCET_REQUESTS: u32 = 6;
const ETH_FAUCET_SLEEP: time::Duration = time::Duration::from_secs(2);
const INIT_ETH_SLEEP: time::Duration = time::Duration::from_secs(15);
const ETH_FAUCET_ADDRESS_ENV_VAR: &str = "ETH_FAUCET_ADDRESS";
const DEFAULT_ETH_FAUCET_ADDRESS: &str = "http://faucet.testnet.golem.network:4000/donate";

pub struct EthFaucetConfig {
    faucet_address: hyper::Uri,
}

impl EthFaucetConfig {
    pub fn from_env() -> PaymentDriverResult<Self> {
        let faucet_address_str = env::var(ETH_FAUCET_ADDRESS_ENV_VAR)
            .ok()
            .unwrap_or_else(|| DEFAULT_ETH_FAUCET_ADDRESS.to_string());
        let faucet_address = faucet_address_str.parse().map_err(|e| {
            PaymentDriverError::LibraryError(format!("invalid faucet address: {}", e))
        })?;
        Ok(EthFaucetConfig { faucet_address })
    }

    pub async fn request_eth(&self, address: Address) -> PaymentDriverResult<()> {
        log::debug!("request eth");
        let client = hyper::Client::new();
        let request_url = format!("{}/{}", &self.faucet_address, utils::addr_to_str(address));

        async fn try_request_eth(
            client: &hyper::Client<HttpConnector, Body>,
            url: &str,
        ) -> PaymentDriverResult<()> {
            let uri: Uri = url.parse().map_err(|e: InvalidUri| {
                PaymentDriverError::LibraryError(format!("URL parse() error: {}", e.to_string()))
            })?;
            let body = client
                .get(uri)
                .await
                .map_err(|e| {
                    PaymentDriverError::LibraryError(format!(
                        "Faucet request - Send() error: {}",
                        e.to_string()
                    ))
                })?
                .body_mut()
                .data()
                .await
                .ok_or(PaymentDriverError::LibraryError(String::from(
                    "Faucet request returned empty response...",
                )))?
                .map_err(|e| {
                    PaymentDriverError::LibraryError(format!(
                        "Faucet request - Body() error: {}",
                        e.to_string()
                    ))
                })?;
            let resp = std::string::String::from_utf8_lossy(body.as_ref());
            if resp.contains("sufficient funds") || resp.contains("txhash") {
                log::debug!("resp: {}", resp);
                return Ok(());
            }

            Err(PaymentDriverError::LibraryError(resp.into_owned()))
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
        Err(PaymentDriverError::LibraryError(format!(
            "Cannot request Eth from Faucet"
        )))
    }
}
