/*
    Top up new accounts from the rinkeby zksync faucet and wait for the funds to arive.
*/

// External crates
use bigdecimal::BigDecimal;
use chrono::{Duration, Utc};
use lazy_static::lazy_static;
use std::{env, time};
use tokio::time::delay_for;

// Workspace uses
use ya_payment_driver::{db::models::Network, model::GenericError};

// Local uses
use crate::zksync::wallet::account_balance;

const DEFAULT_FAUCET_ADDR: &str = "http://3.249.139.167:5778/zk/donatex";
const MAX_FAUCET_REQUESTS: u32 = 6;

lazy_static! {
    static ref FAUCET_ADDR: String =
        env::var("ZKSYNC_FAUCET_ADDR").unwrap_or(DEFAULT_FAUCET_ADDR.to_string());
    static ref MIN_BALANCE: BigDecimal = BigDecimal::from(50);
    static ref MAX_WAIT: Duration = Duration::minutes(1);
}

pub async fn request_ngnt(address: &str, network: Network) -> Result<(), GenericError> {
    let balance = account_balance(address, network).await?;
    if balance >= *MIN_BALANCE {
        return Ok(());
    }

    log::info!(
        "Requesting NGNT from zkSync faucet... address = {}",
        address
    );

    for i in 0..MAX_FAUCET_REQUESTS {
        match faucet_donate(address, network).await {
            Ok(()) => break,
            Err(e) => {
                // Do not warn nor sleep at the last try.
                if i >= MAX_FAUCET_REQUESTS - 1 {
                    log::error!(
                        "Failed to request NGNT from Faucet, tried {} times.: {:?}",
                        MAX_FAUCET_REQUESTS,
                        e
                    );
                    return Err(e);
                } else {
                    log::warn!(
                        "Retrying ({}/{}) to request NGNT from Faucet after failure: {:?}",
                        i + 1,
                        MAX_FAUCET_REQUESTS,
                        e
                    );
                    delay_for(time::Duration::from_secs(10)).await;
                }
            }
        }
    }
    wait_for_ngnt(address, network).await?;
    Ok(())
}

async fn wait_for_ngnt(address: &str, network: Network) -> Result<(), GenericError> {
    log::info!("Waiting for NGNT from faucet...");
    let wait_until = Utc::now() + *MAX_WAIT;
    while Utc::now() < wait_until {
        if account_balance(address, network).await? >= *MIN_BALANCE {
            log::info!("Received NGNT from faucet.");
            return Ok(());
        }
        delay_for(time::Duration::from_secs(3)).await;
    }
    let msg = "Waiting for NGNT timed out.";
    log::error!("{}", msg);
    Err(GenericError::new(msg))
}

async fn faucet_donate(address: &str, _network: Network) -> Result<(), GenericError> {
    // TODO: Reduce timeout to 20-30 seconds when transfer is used.
    let client = awc::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .finish();
    let response = client
        .get(format!("{}/{}", *FAUCET_ADDR, address))
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
