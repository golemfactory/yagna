/*
    Top up new accounts from the rinkeby zksync faucet and wait for the funds to arive.
*/

// External crates
use bigdecimal::BigDecimal;
use chrono::{Duration, Utc};
use lazy_static::lazy_static;
use std::{env, time};
use tokio::time::sleep;

// Workspace uses
use ya_payment_driver::{db::models::Network, model::GenericError};
use ya_utils_networking::resolver;

// Local uses
use crate::zksync::wallet::account_balance;

const DEFAULT_FAUCET_SRV_PREFIX: &str = "_zk-faucet._tcp";
const FAUCET_ADDR_ENVAR: &str = "ZKSYNC_FAUCET_ADDR";
const MAX_FAUCET_REQUESTS: u32 = 6;

lazy_static! {
    static ref MIN_BALANCE: BigDecimal = BigDecimal::from(50);
    static ref MAX_WAIT: Duration = Duration::minutes(1);
}

pub async fn request_tglm(address: &str, network: Network) -> Result<(), GenericError> {
    let balance = account_balance(address, network).await?;
    if balance >= *MIN_BALANCE {
        return Ok(());
    }

    log::info!(
        "Requesting tGLM from zkSync faucet... address = {}",
        address
    );

    for i in 0..MAX_FAUCET_REQUESTS {
        match faucet_donate(address, network).await {
            Ok(()) => break,
            Err(e) => {
                // Do not warn nor sleep at the last try.
                if i >= MAX_FAUCET_REQUESTS - 1 {
                    log::error!(
                        "Failed to request tGLM from Faucet, tried {} times.: {:?}",
                        MAX_FAUCET_REQUESTS,
                        e
                    );
                    return Err(e);
                } else {
                    log::warn!(
                        "Retrying ({}/{}) to request tGLM from Faucet after failure: {:?}",
                        i + 1,
                        MAX_FAUCET_REQUESTS,
                        e
                    );
                    sleep(time::Duration::from_secs(10)).await;
                }
            }
        }
    }
    wait_for_tglm(address, network).await?;
    Ok(())
}

async fn wait_for_tglm(address: &str, network: Network) -> Result<(), GenericError> {
    log::info!("Waiting for tGLM from faucet...");
    let wait_until = Utc::now() + *MAX_WAIT;
    while Utc::now() < wait_until {
        if account_balance(address, network).await? >= *MIN_BALANCE {
            log::info!("Received tGLM from faucet.");
            return Ok(());
        }
        sleep(time::Duration::from_secs(3)).await;
    }
    let msg = "Waiting for tGLM timed out.";
    log::error!("{}", msg);
    Err(GenericError::new(msg))
}

async fn faucet_donate(address: &str, _network: Network) -> Result<(), GenericError> {
    // TODO: Reduce timeout to 20-30 seconds when transfer is used.
    let client = awc::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .finish();
    let faucet_url = resolve_faucet_url().await?;
    let request_url = format!("{}/{}", faucet_url, address);
    let request_url = resolver::try_resolve_dns_record(&request_url).await;
    debug!("Faucet request url: {}", request_url);
    let response = client
        .get(request_url)
        .send()
        .await
        .map_err(GenericError::new)?
        .body()
        .await
        .map_err(GenericError::new)?;
    let response = String::from_utf8_lossy(response.as_ref());
    log::debug!("Funds requested. Response = {}", response);
    // TODO: Verify tx hash
    Ok(())
}

async fn resolve_faucet_url() -> Result<String, GenericError> {
    match env::var(FAUCET_ADDR_ENVAR) {
        Ok(addr) => Ok(addr),
        _ => {
            let faucet_host = resolver::resolve_yagna_srv_record(DEFAULT_FAUCET_SRV_PREFIX)
                .await
                .map_err(|_| GenericError::new("Faucet SRV record cannot be resolved"))?;

            Ok(format!("http://{}/zk/donatex", faucet_host))
        }
    }
}
