// External crates
use bigdecimal::BigDecimal;
use chrono::{Duration, Utc};
use lazy_static::lazy_static;
use std::str::FromStr;
use std::{env, time};
use zksync::zksync_types::H160;

// Workspace uses
use ya_core_model::driver::GenericError;

// Local uses
use crate::zksync::account_balance;

const DEFAULT_FAUCET_ADDR: &str = "http://3.249.139.167:5778/zk/donatex";

lazy_static! {
    static ref FAUCET_ADDR: String =
        env::var("ZKSYNC_FAUCET_ADDR").unwrap_or(DEFAULT_FAUCET_ADDR.to_string());
    static ref MIN_BALANCE: BigDecimal = BigDecimal::from(50);
    static ref MAX_WAIT: Duration = Duration::minutes(1);
}

pub async fn request_ngnt(address: &str) -> Result<(), GenericError> {
    let addr = H160::from_str(&address[2..]).map_err(GenericError::new)?;
    let balance = account_balance(addr).await?;
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

    wait_for_ngnt(addr).await?;
    Ok(())
}

async fn wait_for_ngnt(addr: H160) -> Result<(), GenericError> {
    log::info!("Waiting for NGNT from faucet...");
    let wait_until = Utc::now() + *MAX_WAIT;
    while Utc::now() < wait_until {
        if account_balance(addr).await? >= *MIN_BALANCE {
            log::info!("Received NGNT from faucet.");
            return Ok(());
        }
        tokio::time::delay_for(time::Duration::from_secs(3)).await;
    }
    let msg = "Waiting for NGNT timed out.";
    log::error!("{}", msg);
    Err(GenericError::new(msg))
}
