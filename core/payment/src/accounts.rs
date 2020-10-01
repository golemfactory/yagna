use crate::DEFAULT_PAYMENT_DRIVER;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;
use ya_core_model::driver::{driver_bus_id, AccountMode, Init};
use ya_core_model::identity;
use ya_service_bus::typed as bus;

fn accounts_path(data_dir: &Path) -> PathBuf {
    match env::var("ACCOUNT_LIST").ok() {
        Some(path) => PathBuf::from(path),
        None => data_dir.join("accounts.json"),
    }
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
pub async fn init_accounts(data_dir: &Path) -> anyhow::Result<()> {
    let accounts_path = accounts_path(data_dir);
    log::debug!(
        "Initializing payment accounts from file {} ...",
        accounts_path.display()
    );
    let text = fs::read(accounts_path).await?;
    let accounts: Vec<Account> = serde_json::from_slice(&text)?;

    for account in accounts {
        init_account(account).await?;
    }
    log::debug!("Payment accounts initialized.");
    Ok(())
}

/// Get default node ID from identity service and save it in `ACCOUNT_LIST` file as default payment account.
/// If `ACCOUNT_LIST` file already exists, do nothing.
pub async fn save_default_account(data_dir: &Path) -> anyhow::Result<()> {
    let accounts_path = accounts_path(data_dir);
    if accounts_path.exists() {
        log::debug!("Accounts file {} already exists.", accounts_path.display());
        return Ok(());
    }

    log::debug!(
        "Saving default payment account to file {} ...",
        accounts_path.display()
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
    fs::write(accounts_path, text).await?;
    log::debug!("Default payment account saved successfully.");
    Ok(())
}
