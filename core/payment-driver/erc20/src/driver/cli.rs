/*
    Driver helper for handling messages from CLI.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates

// Workspace uses
use ya_payment_driver::{
    bus,
    db::models::Network,
    model::{Fund, GenericError, Init},
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

    // TODO: payment_api fails to start due to provider account not unlocked
    // if !self.is_account_active(&address) {
    //     return Err(GenericError::new("Can not init, account not active"));
    // }

    wallet::init_wallet(&msg)
        .timeout(Some(30))
        .await
        .map_err(GenericError::new)??;

    let mode = msg.mode();
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
            wallet::fund(dao, address, network)
                .timeout(Some(300))
                .await
                .map_err(GenericError::new)??;
            format!("Received funds from the faucet. address={}", &address)
        }
        Network::Mainnet => format!(
            r#"Your mainnet erc20 address is {}.

To fund your wallet and be able to pay for your activities on Golem head to
the https://chat.golem.network, join the #funding channel and type /terms
and follow instructions to request GLMs.

Mind that to be eligible you have to run your app at least once on testnet -
- we will verify if that is true so we can avoid people requesting "free GLMs"."#,
            address
        ),
    };

    log::debug!("fund completed");
    Ok(result)
}
