/*
    Driver helper for handling messages from CLI.

    Please limit the logic in this file, use local mods to handle the calls.
*/
use std::time::Duration;
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
use crate::erc20::ethereum::{FUND_WALLET_WAIT_TIME, INIT_WALLET_WAIT_TIME};
use crate::{
    dao::Erc20Dao,
    driver::Erc20Driver,
    erc20::{utils, wallet},
    network, DRIVER_NAME,
};
use std::convert::TryFrom;
use ya_payment_driver::db::models::TransactionStatus;

pub async fn init(driver: &Erc20Driver, msg: Init) -> Result<(), GenericError> {
    log::debug!("init: {:?}", msg);
    let mode = msg.mode();
    let address = msg.address();

    // Ensure account is unlock before initialising send mode
    if mode.contains(AccountMode::SEND) {
        driver.is_account_active(&address)?
    }

    wallet::init_wallet(&driver.dao, &msg)
        .timeout(Some(INIT_WALLET_WAIT_TIME))
        .await
        .map_err(|err| GenericError::new(format!("Init wallet future timed out: {}", err)))??;

    let network = network::network_like_to_network(msg.network());
    let token = network::get_network_token(network, msg.token());
    bus::register_account(
        driver,
        &msg.address(),
        &network.to_string(),
        &token,
        mode,
        msg.batch(),
    )
    .await?;

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
                .timeout(Some(FUND_WALLET_WAIT_TIME))
                .await
                .map_err(|err| {
                    GenericError::new(format!("Fund wallet future timed out: {}", err))
                })??;
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

    if msg.receivers.len() != msg.amounts.len() {
        return Err(GenericError::new(format!(
            "Amounts and receivers has to be the same length {} vs {}",
            msg.receivers.len(),
            msg.amounts.len()
        )));
    }
    if msg.receivers.len() == 0 {
        return Err(GenericError::new(
            "Receiver list (to-address) cannot be empty",
        ));
    }
    let is_multi_payment = msg.receivers.len() > 1;

    let gas_limit = msg.gas_limit;
    let gas_price = msg.gas_price;
    let max_gas_price = msg.max_gas_price;
    let glm_balance = wallet::account_balance(sender_h160, network).await?;

    let nonce = wallet::get_next_nonce(dao, sender_h160, network).await?;
    let gasless = msg.gasless;
    let details_str: String;

    if gasless {
        if is_multi_payment {
            Err(GenericError::new(format!(
                "No support for multipayment and gasless transactions"
            )))
        } else {
            let recipient = msg
                .receivers
                .get(0)
                .ok_or(GenericError::new("receivers cannot be empty"))?
                .clone();
            let amount = msg
                .amounts
                .get(0)
                .ok_or(GenericError::new("amounts cannot be empty"))?
                .clone();

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
        }
    } else {
        let db_tx = if is_multi_payment {
            let details_array = msg
                .receivers
                .into_iter()
                .zip(msg.amounts.into_iter())
                .map(|(recipient, amount)| PaymentDetails {
                    recipient,
                    sender: sender.clone(),
                    amount,
                    date: Some(Utc::now()),
                })
                .collect();
            details_str = format!("{:?}", details_array);

            wallet::make_multi_transfer(
                details_array,
                nonce,
                network,
                gas_price,
                max_gas_price,
                gas_limit,
            )
            .await?
        } else {
            let recipient = msg
                .receivers
                .get(0)
                .ok_or(GenericError::new("receivers cannot be empty"))?
                .clone();
            let amount = msg
                .amounts
                .get(0)
                .ok_or(GenericError::new("amounts cannot be empty"))?
                .clone();

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
            details_str = format!("{:?}", details);

            wallet::make_transfer(
                &details,
                nonce,
                network,
                gas_price,
                max_gas_price,
                gas_limit,
            )
            .await?
        };

        // Check if there is enough ETH for gas
        let human_gas_cost = wallet::has_enough_eth_for_gas(&db_tx, network).await?;

        // Everything ok, put the transaction in the queue
        let tx_id = dao
            .insert_raw_transaction(db_tx)
            .await
            .map_err(GenericError::new)?;

        log::debug!("tx_id={}", tx_id);
        log::info!("{}, gas cost: {}", details_str, human_gas_cost);
        if msg.wait_for_tx {
            let tx_hash = loop {
                log::info!("Waiting for confirmation 10s.");
                tokio::time::delay_for(Duration::from_secs(10)).await;
                let transaction_entity = dao.get_transaction_from_tx(&tx_id).await?;
                match TransactionStatus::try_from(transaction_entity.status)
                    .map_err(GenericError::new)?
                {
                    TransactionStatus::Unused => {}
                    TransactionStatus::Created => {}
                    TransactionStatus::Sent => {}
                    TransactionStatus::Pending => {}
                    TransactionStatus::Confirmed => {
                        break transaction_entity.final_tx;
                    }
                    TransactionStatus::Resend => {}
                    TransactionStatus::ResendAndBumpGas => {}
                    TransactionStatus::ErrorSent => {
                        break None;
                    }
                    TransactionStatus::ErrorOnChain => {
                        break transaction_entity.final_tx;
                    }
                    TransactionStatus::ErrorNonceTooLow => {
                        break None;
                    }
                }
            };
            if let Some(tx_hash) = tx_hash {
                let message = format!("tx_hash: {}", tx_hash);
                Ok(message)
            } else {
                Ok("Cannot extract tx hash. Check yagna logs for details".to_string())
            }
        } else {
            let message = format!("tx_id: {}", tx_id);
            Ok(message)
        }
    }
}
