/*
    Driver helper for handling messages from CLI.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates

// Workspace uses
use ya_payment_driver::model::{AccountMode, DriverInitAccount, GenericError};

// Local uses
use crate::{driver::Erc20NextDriver, network, DRIVER_NAME};

pub async fn init(driver: &Erc20NextDriver, msg: DriverInitAccount) -> Result<(), GenericError> {
    log::debug!("init: {:?}", msg);
    let mode = msg.mode();
    let address = msg.address();

    // Ensure account is unlock before initialising send mode
    if mode.contains(AccountMode::SEND) {
        driver.is_account_active(&address).await?
    }

    let network = network::network_like_to_network(msg.network());
    let token = network::get_network_token(network, msg.token());

    log::info!(
        "Initialised payment account. mode={:?}, address={}, driver={}, network={}, token={}",
        mode,
        &msg.address(),
        DRIVER_NAME,
        network,
        token
    );
    Ok(())
}
