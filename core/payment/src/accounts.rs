use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use ya_core_model::driver::{driver_bus_id, AccountMode, Init};
use ya_service_bus::typed as bus;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Account {
    pub driver: String,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub send: bool,
    pub receive: bool,
}

pub(crate) async fn init_account(account: Account) -> anyhow::Result<()> {
    log::debug!("Initializing payment account {:?}...", account);
    let mut mode = AccountMode::NONE;
    mode.set(AccountMode::SEND, account.send);
    mode.set(AccountMode::RECV, account.receive);
    match bus::service(driver_bus_id(account.driver.clone()))
        .call(Init::new(
            account.address,
            account.network,
            account.token,
            mode,
        ))
        .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            let err_msg = format!(
                "Failed to initialize account on driver: {} due to error {}",
                account.driver, e
            );
            log::error!("{}", err_msg);
            return Err(anyhow!("{}", err_msg));
        }
        Err(e) => {
            let err_msg = format!("Error during GSB call init account - Probably driver {} is not running and receiving messages: {}", account.driver, e);
            log::error!("{}", err_msg);
            return Err(anyhow!("{}", err_msg));
        }
    }
    log::debug!("Account initialized.");
    Ok(())
}
