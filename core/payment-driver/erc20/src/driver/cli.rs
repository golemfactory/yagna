/*
    Driver helper for handling messages from CLI.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates

// Workspace uses
use ya_payment_driver::{
    bus,
    db::models::Network,
    model::{AccountMode, Fund, GenericError, Init},
};
use ya_utils_futures::timeout::IntoTimeoutFuture;

// Local uses
use crate::{
    dao::Erc20Dao,
    driver::Erc20Driver,
    erc20::{utils, wallet},
    network, DRIVER_NAME,
};

pub async fn init(driver: &Erc20Driver, msg: Init) -> Result<(), GenericError> {
    log::debug!("init: {:?}", msg);
    let mode = msg.mode();
    let address = msg.address();

    // Ensure account is unlock before initialising send mode
    if mode.contains(AccountMode::SEND) {
        driver.is_account_active(&address)?
    }

    wallet::init_wallet(&msg)
        .timeout(Some(30))
        .await
        .map_err(GenericError::new)??;

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

pub async fn fund(dao: &Erc20Dao, msg: Fund) -> Result<String, GenericError> {
    log::debug!("fund: {:?}", msg);
    let address = msg.address();
    let network = network::network_like_to_network(msg.network());
    let result = match network {
        Network::Rinkeby => {
            let address = utils::str_to_addr(&address)?;
            log::info!(
                "Handling fund request. network={}, address={}",
                &network,
                &address
            );
            wallet::fund(dao, address, network)
                .timeout(Some(60)) // Regular scenario =~ 30s
                .await
                .map_err(GenericError::new)??;
            format!("Received funds from the faucet. address=0x{:x}", &address)
        }
        Network::Mainnet => format!(
            r#"Your mainnet ethereum address is {}.

Send some GLM tokens and ETH for gas to this address to be able to use this driver.

Using this driver is not recommended.
If you want to easily acquire some GLM to try Golem on mainnet please use zksync driver."#,
            address
        ),
    };

    log::debug!("fund completed");
    Ok(result)
}
