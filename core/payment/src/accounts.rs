use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
use tokio::fs;
use ya_core_model::driver::{driver_bus_id, AccountMode, Init};
use ya_core_model::identity;
use ya_service_bus::typed as bus;

pub const DEFAULT_PAYMENT_DRIVER: &str = "ngnt";

lazy_static! {
    pub static ref ACCOUNT_LIST_PATH: String =
        env::var("ACCOUNT_LIST").unwrap_or("accounts.json".to_string());
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Account {
    pub driver: String,
    pub address: String,
    pub send: bool,
    pub receive: bool,
}

pub(crate) async fn init_account(account: Account) -> anyhow::Result<()> {
    log::debug!("Initializing payment account {:?}...", account);
    let mut mode = AccountMode::NONE;
    mode.set(AccountMode::SEND, account.send);
    mode.set(AccountMode::RECV, account.receive);
    bus::service(driver_bus_id(account.driver))
        .call(Init::new(account.address, mode))
        .await??;
    log::debug!("Account initialized.");
    Ok(())
}

/// Read payment accounts information from `ACCOUNT_LIST` file and initialize them.
pub async fn init_accounts() -> anyhow::Result<()> {
    log::debug!(
        "Initializing payment accounts from file {} ...",
        &*ACCOUNT_LIST_PATH
    );
    let text = fs::read(&*ACCOUNT_LIST_PATH).await?;
    let accounts: Vec<Account> = serde_json::from_slice(&text)?;

    for account in accounts {
        init_account(account).await?;
    }
    log::debug!("Payment accounts initialized.");
    Ok(())
}

/// Get default node ID from identity service and save it in `ACCOUNT_LIST` file as default payment account.
/// If `ACCOUNT_LIST` file already exists, do nothing.
pub async fn save_default_account() -> anyhow::Result<()> {
    if Path::new(&*ACCOUNT_LIST_PATH).exists() {
        log::debug!("Accounts file {} already exists.", &*ACCOUNT_LIST_PATH);
        return Ok(());
    }

    log::debug!(
        "Saving default payment account to file {} ...",
        &*ACCOUNT_LIST_PATH
    );
    let default_node_id = bus::service(identity::BUS_ID)
        .call(identity::Get::ByDefault)
        .await??
        .ok_or(anyhow::anyhow!("Default identity not found"))?
        .node_id;
    let default_account = vec![Account {
        driver: DEFAULT_PAYMENT_DRIVER.to_string(),
        address: default_node_id.to_string(),
        send: false,
        receive: true,
    }];
    let text = serde_json::to_string(&default_account)?;
    fs::write(&*ACCOUNT_LIST_PATH, text).await?;
    log::debug!("Default payment account saved successfully.");
    Ok(())
}
