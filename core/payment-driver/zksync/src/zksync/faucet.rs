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
use ya_payment_driver::model::GenericError;

// Local uses
use crate::zksync::wallet::account_balance;

const DEFAULT_FAUCET_ADDR: &str = "http://3.249.139.167:5778/zk/donatex";

lazy_static! {
    static ref FAUCET_ADDR: String =
        env::var("ZKSYNC_FAUCET_ADDR").unwrap_or(DEFAULT_FAUCET_ADDR.to_string());
    static ref MIN_BALANCE: BigDecimal = BigDecimal::from(50);
    static ref MAX_WAIT: Duration = Duration::minutes(1);
}

pub async fn request_ngnt(address: &str) -> Result<(), GenericError> {
    let balance = account_balance(address).await?;
    if balance >= *MIN_BALANCE {
        return Ok(());
    }

    log::info!(
        "Requesting NGNT from zkSync faucet... address = {}",
        address
    );
    let client = awc::Client::new();
    let response = client
        .get(format!("{}/{}", *FAUCET_ADDR, address))
        .send()
        .await
        .map_err(GenericError::new)?
        .body()
        .await
        .map_err(GenericError::new)?;
    let response = String::from_utf8_lossy(response.as_ref());
    log::info!("Funds requested. Response = {}", response);
    // TODO: Verify tx hash

    wait_for_ngnt(address).await?;
    Ok(())
}

async fn wait_for_ngnt(address: &str) -> Result<(), GenericError> {
    log::info!("Waiting for NGNT from faucet...");
    let wait_until = Utc::now() + *MAX_WAIT;
    while Utc::now() < wait_until {
        if account_balance(address).await? >= *MIN_BALANCE {
            log::info!("Received NGNT from faucet.");
            return Ok(());
        }
        delay_for(time::Duration::from_secs(3)).await;
    }
    let msg = "Waiting for NGNT timed out.";
    log::error!("{}", msg);
    Err(GenericError::new(msg))
}
