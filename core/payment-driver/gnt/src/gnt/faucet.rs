use crate::utils;
use crate::{GNTDriverError, GNTDriverResult};
use bigdecimal::BigDecimal;
use chrono::Utc;
use core::num::TryFromIntError;
use ethereum_types::Address;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::{cmp::min, env, time};
use ya_utils_networking::resolver;

const MAX_ETH_FAUCET_REQUESTS: u32 = 6;
const ETH_FAUCET_SLEEP: time::Duration = time::Duration::from_secs(2);
const ETH_FAUCET_ADDRESS_ENV_VAR: &str = "ETH_FAUCET_ADDRESS";
const DEFAULT_ETH_FAUCET_ADDRESS: &str = "http://faucet.testnet.golem.network:4000/donate";

#[derive(Serialize, Deserialize)]
struct FaucetResponse {
    paydate: u64,
    address: String,
    amount: BigDecimal,
}

pub struct EthFaucetConfig {
    faucet_address: awc::http::Uri,
}

impl EthFaucetConfig {
    pub async fn from_env() -> GNTDriverResult<Self> {
        let faucet_address_str = env::var(ETH_FAUCET_ADDRESS_ENV_VAR)
            .ok()
            .unwrap_or_else(|| DEFAULT_ETH_FAUCET_ADDRESS.to_string());
        let faucet_address_str = resolver::try_resolve_dns_record(&faucet_address_str).await;
        let faucet_address = faucet_address_str
            .parse()
            .map_err(|e| GNTDriverError::LibraryError(format!("invalid faucet address: {}", e)))?;
        Ok(EthFaucetConfig { faucet_address })
    }

    pub async fn request_eth(&self, address: Address) -> GNTDriverResult<()> {
        log::info!("Requesting Eth from faucet");
        let client = awc::Client::new();
        let request_url = format!("{}/{}", &self.faucet_address, utils::addr_to_str(address));
        log::debug!("faucet request url: {}", request_url);

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
            log::debug!("raw faucet response: {}", resp);
            if resp.contains("sufficient funds") || resp.contains("txhash") {
                return Ok(());
            } else if resp.contains("paydate") {
                let resp_obj: FaucetResponse = serde_json::from_str(&resp)
                    .map_err(|e| GNTDriverError::LibraryError(e.to_string()))?;
                // Convert miliseconds timestamp to seconds
                let sleep_till = resp_obj.paydate / 1000;
                let current: u64 = Utc::now()
                    .timestamp()
                    .try_into()
                    .map_err(|e: TryFromIntError| GNTDriverError::LibraryError(e.to_string()))?;
                log::debug!("faucet paydate={}, current={}", sleep_till, current);
                // Ignore times in the past
                if sleep_till >= current {
                    // Cap max seconds to wait at 60
                    let capped_seconds = min(sleep_till - current, 60);
                    log::info!(
                        "Waiting for Eth faucet next donation in {}s",
                        capped_seconds
                    );
                    let capped_seconds = time::Duration::from_secs(capped_seconds);
                    tokio::time::sleep(capped_seconds).await;
                    return Err(GNTDriverError::LibraryError(
                        "faucet request is queued, try again".to_string(),
                    ));
                };
            }

            Err(GNTDriverError::LibraryError(resp.into_owned()))
        }

        for i in 0..MAX_ETH_FAUCET_REQUESTS {
            if let Err(e) = try_request_eth(&client, &request_url).await {
                // Do not warn nor sleep at the last try.
                if i >= MAX_ETH_FAUCET_REQUESTS - 1 {
                    log::error!(
                        "Failed to request Eth from Faucet, tried {} times.: {:?}",
                        MAX_ETH_FAUCET_REQUESTS,
                        e
                    );
                } else {
                    log::warn!(
                        "Retrying ({}/{}) to request Eth from Faucet after failure: {:?}",
                        i + 1,
                        MAX_ETH_FAUCET_REQUESTS,
                        e
                    );
                    tokio::time::sleep(ETH_FAUCET_SLEEP).await;
                }
            } else {
                log::info!("Successfully requested Eth.");
                return Ok(());
            }
        }
        Err(GNTDriverError::LibraryError(format!(
            "Cannot request Eth from Faucet"
        )))
    }
}
