use chrono::{DateTime, Duration, Utc};
/*
    Erc20Driver to handle payments on the erc20next network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use erc20_payment_lib::faucet_client::faucet_donate;
use erc20_payment_lib::model::{TokenTransferDao, TxDao};
use erc20_payment_lib::runtime::{
    PaymentRuntime, TransferArgs, TransferType, VerifyTransactionResult,
};
use erc20_payment_lib::utils::{DecimalConvExt, U256ConvExt};
use erc20_payment_lib::{DriverEvent, DriverEventContent};
use ethereum_types::H160;
use ethereum_types::U256;
use num_bigint::BigInt;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::Receiver;
use uuid::Uuid;
use web3::types::H256;
use ya_client_model::payment::DriverStatusProperty;

use ya_payment_driver::{
    account::{Accounts, AccountsArc},
    bus,
    driver::{
        async_trait, BigDecimal, IdentityError, IdentityEvent, Network as NetworkConfig,
        PaymentDriver,
    },
    model::*,
};

// Local uses
use crate::erc20::utils;
use crate::erc20::utils::{big_dec_to_u256, u256_to_big_dec};
use crate::network::platform_to_currency;
use crate::{driver::PaymentDetails, network};
use crate::{network::SUPPORTED_NETWORKS, DRIVER_NAME, RINKEBY_NETWORK};

mod cli;

pub struct Erc20NextDriver {
    active_accounts: AccountsArc,
    payment_runtime: PaymentRuntime,
}

impl Erc20NextDriver {
    pub fn new(payment_runtime: PaymentRuntime, recv: Receiver<DriverEvent>) -> Arc<Self> {
        let this = Arc::new(Self {
            active_accounts: Accounts::new_rc(),
            payment_runtime,
        });

        let this_ = Arc::clone(&this);
        tokio::task::spawn_local(Self::payment_confirm_job(this_, recv));

        this
    }

    pub async fn load_active_accounts(&self) {
        log::debug!("load_active_accounts");
        let unlocked_accounts = bus::list_unlocked_identities().await.unwrap();
        let mut accounts = self.active_accounts.lock().await;
        for account in unlocked_accounts {
            log::debug!("account={}", account);
            accounts.add_account(account)
        }
    }

    async fn is_account_active(&self, address: &str) -> Result<(), GenericError> {
        match self
            .active_accounts
            .as_ref()
            .lock()
            .await
            .get_node_id(address)
        {
            Some(_) => Ok(()),
            None => Err(GenericError::new(format!(
                "Account not active: {}",
                &address
            ))),
        }
    }

    async fn do_transfer(
        &self,
        sender: &str,
        to: &str,
        amount: &BigDecimal,
        network: &str,
        deadline: Option<DateTime<Utc>>,
    ) -> Result<String, GenericError> {
        self.is_account_active(sender).await?;
        let sender = H160::from_str(sender)
            .map_err(|err| GenericError::new(format!("Error when parsing sender {err:?}")))?;
        let receiver = H160::from_str(to)
            .map_err(|err| GenericError::new(format!("Error when parsing receiver {err:?}")))?;
        let amount = big_dec_to_u256(amount)?;

        let payment_id = Uuid::new_v4().to_simple().to_string();

        self.payment_runtime
            .transfer(TransferArgs {
                chain_name: network.to_string(),
                from: sender,
                receiver,
                tx_type: TransferType::Token,
                amount,
                payment_id: payment_id.clone(),
                deadline,
            })
            .await
            .map_err(|err| GenericError::new(format!("Error when inserting transfer {err:?}")))?;

        Ok(payment_id)
    }

    async fn payment_confirm_job(this: Arc<Self>, mut events: Receiver<DriverEvent>) {
        while let Some(event) = events.recv().await {
            match &event.content {
                DriverEventContent::TransferFinished(transfer_finished) => {
                    match this
                        ._confirm_payments(
                            &transfer_finished.token_transfer_dao,
                            &transfer_finished.tx_dao,
                        )
                        .await
                    {
                        Ok(_) => log::info!(
                            "Payment confirmed: {}",
                            transfer_finished
                                .token_transfer_dao
                                .payment_id
                                .clone()
                                .unwrap_or_default()
                        ),
                        Err(e) => log::error!(
                            "Error confirming payment: {}, error: {}",
                            transfer_finished
                                .token_transfer_dao
                                .payment_id
                                .clone()
                                .unwrap_or_default(),
                            e
                        ),
                    }
                }
                DriverEventContent::StatusChanged(_) => {
                    if let Ok(status) = this._status(DriverStatus { network: None }).await {
                        log::info!("Payment driver [{DRIVER_NAME}] status changed: {status:#?}");
                        bus::status_changed(status).await.ok();
                    }
                }
                _ => {}
            }
        }
    }

    async fn _status(
        &self,
        msg: DriverStatus,
    ) -> Result<Vec<DriverStatusProperty>, DriverStatusError> {
        use erc20_payment_lib::StatusProperty as LibStatusProperty;

        // Map chain-id to network
        let chain_id_to_net = |id: i64| self.payment_runtime.network_name(id).unwrap().to_string();

        // check if network matches the filter
        let network_filter = |net_candidate: &str| {
            msg.network
                .as_ref()
                .map(|net| net == net_candidate)
                .unwrap_or(true)
        };

        if let Some(network) = msg.network.as_ref() {
            let found_net = self
                .payment_runtime
                .chains()
                .into_iter()
                .any(|id| &chain_id_to_net(id) == network);

            if !found_net {
                return Err(DriverStatusError::NetworkNotFound(network.clone()));
            }
        }

        Ok(self
            .payment_runtime
            .get_status()
            .await
            .into_iter()
            .flat_map(|prop| match prop {
                LibStatusProperty::InvalidChainId { chain_id } => {
                    Some(DriverStatusProperty::InvalidChainId {
                        driver: DRIVER_NAME.into(),
                        chain_id,
                    })
                }
                LibStatusProperty::CantSign { chain_id, address } => {
                    let network = chain_id_to_net(chain_id);
                    network_filter(&network).then(|| DriverStatusProperty::CantSign {
                        driver: DRIVER_NAME.into(),
                        network,
                        address,
                    })
                }
                LibStatusProperty::NoGas {
                    chain_id,
                    address,
                    missing_gas,
                } => {
                    let network = chain_id_to_net(chain_id);
                    network_filter(&network).then(|| DriverStatusProperty::InsufficientGas {
                        driver: DRIVER_NAME.into(),
                        address,
                        network,
                        needed_gas_est: missing_gas.to_string(),
                    })
                }
                LibStatusProperty::NoToken {
                    chain_id,
                    address,
                    missing_token,
                } => {
                    let network = chain_id_to_net(chain_id);
                    network_filter(&network).then(|| DriverStatusProperty::InsufficientToken {
                        driver: DRIVER_NAME.into(),
                        address,
                        network,
                        needed_token_est: missing_token.to_string(),
                    })
                }
                LibStatusProperty::TxStuck { chain_id } => {
                    let network = chain_id_to_net(chain_id);
                    network_filter(&network).then(|| DriverStatusProperty::TxStuck {
                        driver: DRIVER_NAME.into(),
                        network,
                    })
                }
                LibStatusProperty::Web3RpcError { chain_id, .. } => {
                    let network = chain_id_to_net(chain_id);
                    network_filter(&network).then(|| DriverStatusProperty::RpcError {
                        driver: DRIVER_NAME.into(),
                        network,
                    })
                }
            })
            .collect())
    }

    async fn _confirm_payments(
        &self,
        token_transfer: &TokenTransferDao,
        tx: &TxDao,
    ) -> Result<(), GenericError> {
        log::debug!("Received event TransferFinished: {:#?}", token_transfer);

        let chain_id = token_transfer.chain_id;
        let network_name = &self
            .payment_runtime
            .network_name(chain_id)
            .ok_or(GenericError::new(format!(
                "Missing configuration for chain_id {chain_id}"
            )))?
            .to_string();

        let networks = self.get_networks();
        let network = networks.get(network_name).ok_or(GenericError::new(format!(
            "Network {network_name} not supported by Erc20NextDriver"
        )))?;
        let platform = network
            .tokens
            .get(&network.default_token)
            .ok_or(GenericError::new(format!(
                "Network {} doesn't specify platform for default token {}",
                network_name, network.default_token
            )))?
            .as_str();

        let Ok(tx_token_amount) = U256::from_dec_str(&token_transfer.token_amount) else {
            return Err(GenericError::new(format!(
                "Malformed token_transfer.token_amount: {}",
                token_transfer.token_amount
            )));
        };
        let Ok(tx_token_amount) = u256_to_big_dec(tx_token_amount) else {
            return Err(GenericError::new(format!(
                "Cannot convert to big decimal tx_token_amount: {}",
                tx_token_amount
            )));
        };
        let payment_details = PaymentDetails {
            recipient: token_transfer.receiver_addr.clone(),
            sender: token_transfer.from_addr.clone(),
            amount: tx_token_amount,
            date: token_transfer.paid_date,
        };

        let tx_hash = tx.tx_hash.clone().ok_or(GenericError::new(format!(
            "Missing tx_hash in tx_dao: {:?}",
            tx
        )))?;
        if tx_hash.len() != 66 {
            return Err(GenericError::new(format!(
                "Malformed tx_hash, length should be 66: {:?}",
                tx_hash
            )));
        };
        let transaction_hash = hex::decode(&tx_hash[2..]).map_err(|err| {
            GenericError::new(format!("Malformed tx.tx_hash: {:?} {err}", tx_hash))
        })?;

        log::info!("name: {}", &self.get_name());
        log::info!("platform: {}", platform);
        log::info!("order_id: {}", token_transfer.payment_id.as_ref().unwrap());
        log::info!("payment_details: {}", payment_details);
        log::info!("confirmation: 0x{}", hex::encode(&transaction_hash));

        let Some(payment_id) = &token_transfer.payment_id else {
            return Err(GenericError::new("token_transfer.payment_id is null"));
        };
        bus::notify_payment(
            &self.get_name(),
            platform,
            vec![payment_id.clone()],
            &payment_details,
            transaction_hash,
        )
        .await
    }
}

#[async_trait(?Send)]
impl PaymentDriver for Erc20NextDriver {
    async fn account_event(
        &self,
        _caller: String,
        msg: IdentityEvent,
    ) -> Result<(), IdentityError> {
        self.active_accounts.lock().await.handle_event(msg);
        Ok(())
    }

    async fn enter(&self, _caller: String, msg: Enter) -> Result<String, GenericError> {
        log::info!("ENTER = Not Implemented: {:?}", msg);
        Ok("NOT_IMPLEMENTED".to_string())
    }

    async fn exit(&self, _caller: String, msg: Exit) -> Result<String, GenericError> {
        log::info!("EXIT = Not Implemented: {:?}", msg);
        Ok("NOT_IMPLEMENTED".to_string())
    }

    async fn get_account_balance(
        &self,
        _caller: String,
        msg: GetAccountBalance,
    ) -> Result<BigDecimal, GenericError> {
        let platform = msg.platform();
        let network = platform.split('-').nth(1).ok_or(GenericError::new(format!(
            "Malformed platform string: {}",
            msg.platform()
        )))?;

        let address_str = msg.address();
        let address = H160::from_str(&address_str).map_err(|e| {
            GenericError::new(format!("{} isn't a valid H160 address: {}", address_str, e))
        })?;

        log::debug!(
            "Getting balance for network: {}, address: {}",
            network.to_string(),
            address_str
        );

        let balance = self
            .payment_runtime
            .get_token_balance(network.to_string(), address)
            .await
            .map_err(|e| GenericError::new(e.to_string()))?;
        let balance_int = BigInt::from_str(&format!("{balance}")).unwrap();

        Ok(BigDecimal::new(balance_int, 18))
    }

    async fn get_account_gas_balance(
        &self,
        _caller: String,
        msg: GetAccountGasBalance,
    ) -> Result<Option<GasDetails>, GenericError> {
        let platform = msg.platform();
        let network = platform.split('-').nth(1).ok_or(GenericError::new(format!(
            "Malformed platform string: {}",
            msg.platform()
        )))?;

        let address_str = msg.address();
        let address = H160::from_str(&address_str).map_err(|e| {
            GenericError::new(format!("{} isn't a valid H160 address: {}", address_str, e))
        })?;

        let balance = self
            .payment_runtime
            .get_gas_balance(network.to_string(), address)
            .await
            .map_err(|e| GenericError::new(e.to_string()))?;
        let balance_int = BigInt::from_str(&format!("{balance}")).unwrap();
        let balance = BigDecimal::new(balance_int, 18);

        let (currency_short_name, currency_long_name) = platform_to_currency(platform)?;

        Ok(Some(GasDetails {
            currency_long_name,
            currency_short_name,
            balance,
        }))
    }

    fn get_name(&self) -> String {
        DRIVER_NAME.to_string()
    }

    fn get_default_network(&self) -> String {
        RINKEBY_NETWORK.to_string()
    }

    fn get_networks(&self) -> HashMap<String, NetworkConfig> {
        SUPPORTED_NETWORKS.clone()
    }

    fn recv_init_required(&self) -> bool {
        false
    }

    async fn init(&self, _caller: String, msg: Init) -> Result<Ack, GenericError> {
        cli::init(self, msg).await?;
        Ok(Ack {})
    }

    async fn fund(&self, _caller: String, msg: Fund) -> Result<String, GenericError> {
        log::debug!("fund: {:?}", msg);
        let address = msg.address();
        let network = network::network_like_to_network(msg.network());
        let result = {
            let address = utils::str_to_addr(&address)?;
            log::info!(
                "Handling fund request. network={}, address={:#x}",
                &network,
                &address
            );
            let chain_cfg = self
                .payment_runtime
                .setup
                .chain_setup
                .get(&(network as i64))
                .ok_or(GenericError::new(format!(
                    "Missing chain config for network {}",
                    network
                )))?;
            let _mint_contract_address =
                chain_cfg
                    .faucet_setup
                    .mint_glm_address
                    .ok_or(GenericError::new(format!(
                        "Missing mint contract address for network {}",
                        network
                    )))?;
            let mint_min_glm_allowed =
                chain_cfg
                    .faucet_setup
                    .mint_max_glm_allowed
                    .ok_or(GenericError::new(format!(
                        "Missing mint min glm allowed for network {}",
                        network
                    )))?;
            let faucet_client_max_eth_allowed = chain_cfg
                .faucet_setup
                .client_max_eth_allowed
                .ok_or(GenericError::new(format!(
                    "Missing faucet client max eth allowed for network {}",
                    network
                )))?;

            let starting_eth_balance = match self
                .payment_runtime
                .get_gas_balance(network.to_string(), address)
                .await
            {
                Ok(balance) => {
                    log::info!("Gas balance is {}", balance.to_eth_str());
                    balance
                }
                Err(err) => {
                    log::error!("Error getting gas balance: {}", err);
                    return Err(GenericError::new(format!(
                        "Error getting gas balance: {}",
                        err
                    )));
                }
            };
            let starting_glm_balance = match self
                .payment_runtime
                .get_token_balance(network.to_string(), address)
                .await
            {
                Ok(balance) => {
                    log::info!("tGLM balance is {}", balance.to_eth_str());
                    balance
                }
                Err(err) => {
                    log::error!("Error getting tGLM balance: {}", err);
                    return Err(GenericError::new(format!(
                        "Error getting tGLM balance: {}",
                        err
                    )));
                }
            };

            let faucet_srv_prefix =
                chain_cfg
                    .faucet_setup
                    .client_srv
                    .clone()
                    .ok_or(GenericError::new(format!(
                        "Missing faucet_srv_port for network {}",
                        network
                    )))?;
            let faucet_lookup_domain =
                chain_cfg
                    .faucet_setup
                    .lookup_domain
                    .clone()
                    .ok_or(GenericError::new(format!(
                        "Missing faucet_lookup_domain for network {}",
                        network
                    )))?;
            let faucet_srv_port =
                chain_cfg
                    .faucet_setup
                    .srv_port
                    .ok_or(GenericError::new(format!(
                        "Missing faucet_srv_port for network {}",
                        network
                    )))?;
            let faucet_host =
                chain_cfg
                    .faucet_setup
                    .client_host
                    .clone()
                    .ok_or(GenericError::new(format!(
                        "Missing faucet_host for network {}",
                        network
                    )))?;

            let eth_received = if starting_eth_balance
                < faucet_client_max_eth_allowed
                    .to_u256_from_eth()
                    .map_err(|err| {
                        GenericError::new(format!(
                            "faucet_client_max_eth_allowed failed to convert {}",
                            err
                        ))
                    })? {
                match faucet_donate(
                    &faucet_srv_prefix,
                    &faucet_lookup_domain,
                    &faucet_host,
                    faucet_srv_port,
                    address,
                )
                .await
                {
                    Ok(_) => {
                        log::info!("Faucet donation successful");
                    }
                    Err(e) => {
                        log::error!("Error donating from faucet: {}", e);
                    }
                }
                let time_now = Instant::now();
                let mut iteration = -1;
                loop {
                    iteration += 1;
                    if iteration == 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    } else {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    if time_now.elapsed().as_secs() > 120 {
                        log::error!(
                            "Faucet donation not received after {} seconds",
                            time_now.elapsed().as_secs()
                        );
                        return Err(GenericError::new(format!(
                            "Faucet donation not received after {} seconds",
                            time_now.elapsed().as_secs()
                        )));
                    }
                    match self
                        .payment_runtime
                        .get_gas_balance(network.to_string(), address)
                        .await
                    {
                        Ok(current_balance) => {
                            if current_balance > starting_eth_balance {
                                log::info!(
                                    "Received {} ETH from faucet",
                                    (current_balance - starting_eth_balance).to_eth_str()
                                );
                                break current_balance - starting_eth_balance;
                            } else {
                                log::info!("Waiting for ETH from faucet. Current balance: {}. Elapsed: {}/{}", current_balance.to_eth_str(), time_now.elapsed().as_secs(), 120);
                            }
                        }
                        Err(err) => {
                            log::error!("Error getting gas balance: {}", err);
                        }
                    }
                }
            } else {
                log::info!(
                    "ETH balance is {} which is more than {} allowed by faucet",
                    starting_eth_balance.to_eth_str(),
                    faucet_client_max_eth_allowed
                );
                U256::zero()
            };

            let glm_received = if starting_glm_balance
                < mint_min_glm_allowed.to_u256_from_eth().map_err(|err| {
                    GenericError::new(format!("mint_min_glm_allowed failed to convert {}", err))
                })? {
                match self
                    .payment_runtime
                    .mint_golem_token(&network.to_string(), address)
                    .await
                {
                    Ok(_) => {
                        log::info!("Added mint tGLM transaction to queue {}", address);
                    }
                    Err(e) => {
                        log::error!("Error minting tGLM tokens for address {}: {}", address, e);
                    }
                }
                let time_now = Instant::now();
                let mut iteration = -1;
                loop {
                    iteration += 1;
                    if iteration == 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    } else {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    if time_now.elapsed().as_secs() > 120 {
                        log::error!(
                            "Mint transaction not finished after {} seconds",
                            time_now.elapsed().as_secs()
                        );
                        return Err(GenericError::new(format!(
                            "Mint transaction not finished after {} seconds",
                            time_now.elapsed().as_secs()
                        )));
                    }
                    match self
                        .payment_runtime
                        .get_token_balance(network.to_string(), address)
                        .await
                    {
                        Ok(current_balance) => {
                            if current_balance > starting_glm_balance {
                                log::info!(
                                    "Created {} tGLM using mint transaction",
                                    (current_balance - starting_glm_balance).to_eth_str()
                                );
                                break current_balance - starting_glm_balance;
                            } else {
                                log::info!(
                                    "Waiting for mint result. Current balance: {}. Elapsed: {}/{}",
                                    current_balance.to_eth_str(),
                                    time_now.elapsed().as_secs(),
                                    120
                                );
                            }
                        }
                        Err(err) => {
                            log::error!("Error getting tGLM balance: {}", err);
                        }
                    }
                }
            } else {
                log::info!(
                    "tGLM balance is {} which is more than allowed by GLM minting contract {}",
                    starting_glm_balance.to_eth_str(),
                    mint_min_glm_allowed
                );
                U256::zero()
            };
            let mut str_output = if eth_received > U256::zero() || glm_received > U256::zero() {
                format!(
                    "Successfully received {} ETH and {} tGLM",
                    eth_received.to_eth_str(),
                    glm_received.to_eth_str()
                )
            } else if eth_received > U256::zero() {
                format!("Successfully received {} ETH", eth_received.to_eth_str())
            } else if glm_received > U256::zero() {
                format!("Successfully minted {} tGLM", glm_received.to_eth_str())
            } else {
                "No funds received".to_string()
            };
            let final_eth_balance = match self
                .payment_runtime
                .get_gas_balance(network.to_string(), address)
                .await
            {
                Ok(balance) => {
                    log::info!("Gas balance is {}", balance.to_eth_str());
                    balance
                }
                Err(err) => {
                    log::error!("Error getting gas balance: {}", err);
                    return Err(GenericError::new(format!(
                        "Error getting gas balance: {}",
                        err
                    )));
                }
            };
            str_output += &format!(
                "\nYou have {} tETH and {} tGLM",
                final_eth_balance.to_eth_str(),
                (starting_glm_balance + glm_received).to_eth_str()
            );
            str_output
        };
        log::debug!("fund completed");
        Ok(result)
    }

    async fn transfer(&self, _caller: String, msg: Transfer) -> Result<String, GenericError> {
        let network = msg
            .network
            .ok_or(GenericError::new("Network not specified".to_string()))?;

        self.do_transfer(
            &msg.sender,
            &msg.to,
            &msg.amount,
            &network,
            Some(Utc::now()),
        )
        .await
    }

    async fn schedule_payment(
        &self,
        _caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError> {
        log::debug!("schedule_payment: {:?}", msg);

        let platform = msg.platform();
        let network = platform.split('-').nth(1).ok_or(GenericError::new(format!(
            "Malformed platform string: {}",
            msg.platform()
        )))?;

        let transfer_margin = Duration::minutes(2);

        self.do_transfer(
            &msg.sender(),
            &msg.recipient(),
            &msg.amount(),
            network,
            Some(msg.due_date() - transfer_margin),
        )
        .await
    }

    async fn verify_payment(
        &self,
        _caller: String,
        msg: VerifyPayment,
    ) -> Result<PaymentDetails, GenericError> {
        log::debug!("verify_payment: {:?}", msg);
        let (network, _) = network::platform_to_network_token(msg.platform())?;
        let tx_hash = format!("0x{}", hex::encode(msg.confirmation().confirmation));
        log::info!("Verifying transaction: {} on network {}", tx_hash, network);
        let verify_res = self
            .payment_runtime
            .verify_transaction(
                network as i64,
                H256::from_str(&tx_hash)
                    .map_err(|_| GenericError::new("Hash cannot be converted to string"))?,
                H160::from_str(&msg.details.payer_addr)
                    .map_err(|_| GenericError::new("payer_addr"))?,
                H160::from_str(&msg.details.payee_addr)
                    .map_err(|_| GenericError::new("payer_addr"))?,
                big_dec_to_u256(&msg.details.amount)?,
            )
            .await
            .map_err(|err| GenericError::new(format!("Error verifying transaction: {}", err)))?;

        match verify_res {
            VerifyTransactionResult::Verified { amount } => {
                let amount_int = BigInt::from_str(&format!("{amount}")).unwrap();
                let amount = BigDecimal::new(amount_int, 18);
                Ok(PaymentDetails {
                    recipient: msg.details.payee_addr.clone(),
                    sender: msg.details.payer_addr.clone(),
                    amount,
                    date: None,
                })
            }
            VerifyTransactionResult::Rejected(reason) => Err(GenericError::new(format!(
                "Payment {tx_hash} rejected: {reason}",
            ))),
        }
    }

    async fn validate_allocation(
        &self,
        caller: String,
        msg: ValidateAllocation,
    ) -> Result<bool, GenericError> {
        log::debug!("Validate_allocation: {:?}", msg);
        let account_balance = self
            .get_account_balance(
                caller,
                GetAccountBalance::new(msg.address, msg.platform.clone()),
            )
            .await?;

        let total_allocated_amount: BigDecimal = msg
            .existing_allocations
            .into_iter()
            .filter(|allocation| allocation.payment_platform == msg.platform)
            .map(|allocation| allocation.remaining_amount)
            .sum();

        log::info!(
            "Allocation validation: \
            allocating: {:.5}, \
            account_balance: {:.5}, \
            total_allocated_amount: {:.5}",
            msg.amount,
            account_balance,
            total_allocated_amount,
        );

        Ok(msg.amount <= account_balance - total_allocated_amount)
    }

    async fn status(
        &self,
        _caller: String,
        msg: DriverStatus,
    ) -> Result<Vec<DriverStatusProperty>, DriverStatusError> {
        self._status(msg).await
    }

    async fn shut_down(&self, _caller: String, _msg: ShutDown) -> Result<(), GenericError> {
        // no-op, erc20_payment_lib driver doesn't expose clean shutdown interface yet
        Ok(())
    }
}
