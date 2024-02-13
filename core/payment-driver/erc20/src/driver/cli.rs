/*
    Driver helper for handling messages from CLI.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates

// Workspace uses
use ya_payment_driver::{
    bus,
    model::{AccountMode, GenericError, Init},
};

// Local uses
use crate::{driver::Erc20Driver, network, DRIVER_NAME};

pub async fn init(driver: &Erc20Driver, msg: Init) -> Result<(), GenericError> {
    log::debug!("init: {:?}", msg);
    let mode = msg.mode();
    let address = msg.address();

    // Ensure account is unlock before initialising send mode
    if mode.contains(AccountMode::SEND) {
        driver.is_account_active(&address).await?
    }

    let network = network::network_like_to_network(msg.network());
    let token = network::get_network_token(network, msg.token());
    bus::register_account(driver, &msg.address(), &network.to_string(), &token, mode).await?;

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
