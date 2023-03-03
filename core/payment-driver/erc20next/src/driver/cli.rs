/*
    Driver helper for handling messages from CLI.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use chrono::Utc;

// Workspace uses
use ya_payment_driver::{
    bus,
    db::models::Network,
    model::{AccountMode, Fund, GenericError, Init, PaymentDetails, Transfer},
};
use ya_utils_futures::timeout::IntoTimeoutFuture;

// Local uses
use crate::{
    dao::Erc20Dao,
    driver::Erc20NextDriver,
    erc20::{utils, wallet},
    network, DRIVER_NAME,
};

pub async fn init(driver: &Erc20NextDriver, msg: Init) -> Result<(), GenericError> {
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
        Network::Goerli => format!(
            r#"Your Goerli Polygon address is {}.

Goerli GLM/MATIC faucet is not supported. Please use erc20/rinkeby (`--driver erc20 --network rinkeby`) instead.

To be able to use Goerli Polygon network, please send some GLM tokens and MATIC for gas to this address.
"#,
            address
        ),
        Network::Mumbai => format!(
            r#"Your Mumbai Polygon address is {}.

Mumbai GLM/MATIC faucet is not supported. Please use erc20/rinkeby (`--driver erc20 --network rinkeby`) instead.

To be able to use Mumbai Polygon network, please send some GLM tokens and MATIC for gas to this address.
"#,
            address
        ),
        Network::Polygon => format!(
            r#"Your mainnet Polygon address is {}.

To fund your wallet and be able to pay for your activities on Golem head to
the https://chat.golem.network, join the #funding channel and type /terms
and follow instructions to request GLMs.

Mind that to be eligible you have to run your app at least once on testnet -
- we will verify if that is true so we can avoid people requesting "free GLMs".

You will also need some MATIC for gas. You can acquire them by visiting
  https://macncheese.finance/matic-polygon-mainnet-faucet.php
"#,
            address
        ),
        Network::Mainnet => format!(
            r#"Using this driver is not recommended. Consider using the Polygon driver instead.

Your mainnet ethereum address is {}.
To be able to use mainnet Ethereum driver please send some GLM tokens and ETH for gas to this address.
"#,
            address
        ),
    };

    log::debug!("fund completed");
    Ok(result)
}

pub async fn transfer(dao: &Erc20Dao, msg: Transfer) -> Result<String, GenericError> {
    log::debug!("transfer: {:?}", msg);
    let network = network::network_like_to_network(msg.network);
    let token = network::get_network_token(network, None);
    let sender = msg.sender;
    let sender_h160 = utils::str_to_addr(&sender)?;
    let recipient = msg.to;
    let amount = msg.amount;
    let gas_limit = msg.gas_limit;
    let gas_price = msg.gas_price;
    let max_gas_price = msg.max_gas_price;
    let gasless = msg.gasless;
    let glm_balance = wallet::account_balance(sender_h160, network).await?;

    if amount > glm_balance {
        return Err(GenericError::new(format!(
            "Not enough {} balance for transfer. balance={}, tx_amount={}, address={}, network={}",
            token, glm_balance, amount, sender, network
        )));
    }

    let details = PaymentDetails {
        recipient,
        sender,
        amount,
        date: Some(Utc::now()),
    };

    if gasless {
        let tx_id = wallet::make_gasless_transfer(&details, network).await?;

        let endpoint = match network {
            Network::Polygon => "https://polygonscan.com/tx/",
            Network::Mainnet => "https://etherscan.io/tx/",
            Network::Rinkeby => "https://rinkeby.etherscan.io/tx/",
            Network::Goerli => "https://goerli.etherscan.io/tx/",
            Network::Mumbai => "https://mumbai.polygonscan.com/tx/",
        };

        let message = format!("Follow your transaction: {}0x{:x}", endpoint, tx_id);
        Ok(message)
    } else {
        let nonce = wallet::get_next_nonce(dao, sender_h160, network).await?;

        let db_tx = wallet::make_transfer(
            &details,
            nonce,
            network,
            gas_price,
            max_gas_price,
            gas_limit,
        )
        .await?;

        // Check if there is enough ETH for gas
        let human_gas_cost = wallet::has_enough_eth_for_gas(&db_tx, network).await?;

        // Everything ok, put the transaction in the queue
        let tx_id = dao.insert_raw_transaction(db_tx).await;

        let message = format!(
            "Scheduled {} transfer. details={:?}, max_gas_cost={} ETH, network={}",
            &token, &details, &human_gas_cost, &network
        );
        log::info!("{}", message);
        log::debug!("tx_id={}", tx_id);
        Ok(message)
    }
}
