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
    bus::service(driver_bus_id(account.driver))
        .call(Init::new(
            account.address,
            account.network,
            account.token,
            mode,
        ))
        .await??;
    log::debug!("Account initialized.");
    Ok(())
}
