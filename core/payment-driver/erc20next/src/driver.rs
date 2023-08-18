/*
    Erc20Driver to handle payments on the erc20 network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use chrono::{Duration, Utc};
use erc20_payment_lib::db::ops::insert_token_transfer;
use erc20_payment_lib::runtime::PaymentRuntime;
use erc20_payment_lib::transaction::create_token_transfer;
use ethereum_types::H160;
use futures::lock::Mutex;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

// Workspace uses
use ya_payment_driver::{
    account::{Accounts, AccountsRc},
    bus,
    cron::PaymentDriverCron,
    dao::DbExecutor,
    db::models::Network,
    driver::{
        async_trait, BigDecimal, IdentityError, IdentityEvent, Network as NetworkConfig,
        PaymentDriver,
    },
    model::*,
};

// Local uses
use crate::erc20::utils::big_dec_to_u256;
use crate::{
    dao::Erc20Dao, network::SUPPORTED_NETWORKS, DRIVER_NAME, MUMBAI_PLATFORM,
    POLYGON_MAINNET_PLATFORM, RINKEBY_NETWORK, RINKEBY_PLATFORM, YATESTNET_PLATFORM,
};

mod api;
mod cli;
mod cron;

lazy_static::lazy_static! {
    static ref TX_SENDOUT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(
            std::env::var("ERC20_SENDOUT_INTERVAL_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(30),
        );

    static ref TX_CONFIRMATION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(
            std::env::var("ERC20_CONFIRMATION_INTERVAL_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(30),
        );
}

pub struct Erc20NextDriver {
    active_accounts: AccountsRc,
    dao: Erc20Dao,
    sendout_lock: Mutex<()>,
    confirmation_lock: Mutex<()>,
    payment_runtime: PaymentRuntime,
}

impl Erc20NextDriver {
    pub fn new(db: DbExecutor, pr: PaymentRuntime) -> Self {
        Self {
            active_accounts: Accounts::new_rc(),
            dao: Erc20Dao::new(db),
            sendout_lock: Default::default(),
            confirmation_lock: Default::default(),
            payment_runtime: pr,
        }
    }

    pub async fn load_active_accounts(&self) {
        log::debug!("load_active_accounts");
        let unlocked_accounts = bus::list_unlocked_identities().await.unwrap();
        let mut accounts = self.active_accounts.borrow_mut();
        for account in unlocked_accounts {
            log::debug!("account={}", account);
            accounts.add_account(account)
        }
    }

    fn is_account_active(&self, address: &str) -> Result<(), GenericError> {
        match self.active_accounts.as_ref().borrow().get_node_id(address) {
            Some(_) => Ok(()),
            None => Err(GenericError::new(format!(
                "Account not active: {}",
                &address
            ))),
        }
    }
}

#[async_trait(?Send)]
impl PaymentDriver for Erc20NextDriver {
    async fn account_event(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: IdentityEvent,
    ) -> Result<(), IdentityError> {
        self.active_accounts.borrow_mut().handle_event(msg);
        Ok(())
    }

    async fn enter(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: Enter,
    ) -> Result<String, GenericError> {
        log::info!("ENTER = Not Implemented: {:?}", msg);
        Ok("NOT_IMPLEMENTED".to_string())
    }

    async fn exit(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: Exit,
    ) -> Result<String, GenericError> {
        log::info!("EXIT = Not Implemented: {:?}", msg);
        Ok("NOT_IMPLEMENTED".to_string())
    }

    async fn get_account_balance(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: GetAccountBalance,
    ) -> Result<BigDecimal, GenericError> {
        api::get_account_balance(msg).await
    }

    async fn get_account_gas_balance(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: GetAccountGasBalance,
    ) -> Result<Option<GasDetails>, GenericError> {
        api::get_account_gas_balance(msg).await
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

    async fn init(&self, _db: DbExecutor, _caller: String, msg: Init) -> Result<Ack, GenericError> {
        cli::init(self, msg).await?;
        Ok(Ack {})
    }

    async fn fund(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: Fund,
    ) -> Result<String, GenericError> {
        cli::fund(&self.dao, msg).await
    }

    async fn transfer(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: Transfer,
    ) -> Result<String, GenericError> {
        self.is_account_active(&msg.sender)?;
        let sender = H160::from_str(&msg.sender)
            .map_err(|err| GenericError::new(format!("Error when parsing sender {err:?}")))?;
        let receiver = H160::from_str(&msg.to)
            .map_err(|err| GenericError::new(format!("Error when parsing receiver {err:?}")))?;
        let amount = big_dec_to_u256(&msg.amount)?;
        let network = msg
            .network
            .ok_or(GenericError::new("Network not specified".to_string()))?;
        let chain_id = if network == "rinkeby" {
            4
        } else if network == "polygon" {
            137
        } else if network == "mumbai" {
            80001
        } else if network == "dev" {
            987789
        } else {
            return Err(GenericError::new(format!(
                "Unsupported network: {}",
                network
            )));
        };
        let chain_cfg = self
            .payment_runtime
            .setup
            .chain_setup
            .get(&chain_id)
            .ok_or(GenericError::new(format!(
                "Cannot find chain cfg for chain {chain_id}"
            )))?;

        let glm_address = chain_cfg.glm_address.ok_or(GenericError::new(format!(
            "Cannot find GLM address for chain {chain_id}"
        )))?;

        let payment_id = Uuid::new_v4().to_simple().to_string();
        let token_transfer = create_token_transfer(
            sender,
            receiver,
            chain_cfg.chain_id,
            Some(&payment_id),
            Some(glm_address),
            amount,
        );
        let _res = insert_token_transfer(&self.payment_runtime.conn, &token_transfer)
            .await
            .map_err(|err| GenericError::new(format!("Error when inserting transfer {err:?}")))?;
        //cli::transfer(&self.dao, msg).await
        Ok(payment_id)
    }

    async fn schedule_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError> {
        log::debug!("schedule_payment: {:?}", msg);

        self.is_account_active(&msg.sender())?;
        let sender = H160::from_str(&msg.sender())
            .map_err(|err| GenericError::new(format!("Error when parsing sender {err:?}")))?;
        let receiver = H160::from_str(&msg.recipient())
            .map_err(|err| GenericError::new(format!("Error when parsing receiver {err:?}")))?;
        let amount = big_dec_to_u256(&msg.amount())?;
        let network = msg.platform();
        let chain_id = if network == RINKEBY_PLATFORM {
            4
        } else if network == POLYGON_MAINNET_PLATFORM {
            137
        } else if network == MUMBAI_PLATFORM {
            80001
        } else if network == YATESTNET_PLATFORM {
            987789
        } else {
            return Err(GenericError::new(format!(
                "Unsupported network: {}",
                network
            )));
        };
        let chain_cfg = self
            .payment_runtime
            .setup
            .chain_setup
            .get(&chain_id)
            .ok_or(GenericError::new(format!(
                "Cannot find chain cfg for chain {chain_id}"
            )))?;

        let glm_address = chain_cfg.glm_address.ok_or(GenericError::new(format!(
            "Cannot find GLM address for chain {chain_id}"
        )))?;

        let payment_id = Uuid::new_v4().to_simple().to_string();
        let token_transfer = create_token_transfer(
            sender,
            receiver,
            chain_cfg.chain_id,
            Some(&payment_id),
            Some(glm_address),
            amount,
        );
        let _res = insert_token_transfer(&self.payment_runtime.conn, &token_transfer)
            .await
            .map_err(|err| GenericError::new(format!("Error when inserting transfer {err:?}")))?;
        //cli::transfer(&self.dao, msg).await
        Ok(payment_id)
    }

    async fn verify_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: VerifyPayment,
    ) -> Result<PaymentDetails, GenericError> {
        api::verify_payment(msg).await
    }

    async fn validate_allocation(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: ValidateAllocation,
    ) -> Result<bool, GenericError> {
        api::validate_allocation(msg).await
    }

    async fn shut_down(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: ShutDown,
    ) -> Result<(), GenericError> {
        self.send_out_payments().await;
        // HACK: Make sure that send-out job did complete. It might have just been running in another thread (cron). In such case .send_out_payments() would not block.
        self.sendout_lock.lock().await;
        let timeout = Duration::from_std(msg.timeout)
            .map_err(|e| GenericError::new(format!("Invalid shutdown timeout: {}", e)))?;
        let deadline = Utc::now() + timeout - Duration::seconds(1);
        while {
            self.confirm_payments().await; // Run it at least once
            Utc::now() < deadline && self.dao.has_unconfirmed_txs().await? // Stop if deadline passes or there are no more transactions to confirm
        } {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        Ok(())
    }
}

#[async_trait(?Send)]
impl PaymentDriverCron for Erc20NextDriver {
    async fn confirm_payments(&self) {
        let guard = match self.confirmation_lock.try_lock() {
            None => {
                log::trace!("ERC-20 confirmation job in progress.");
                return;
            }
            Some(guard) => guard,
        };
        log::trace!("Running ERC-20 confirmation job...");
        for network_key in self.get_networks().keys() {
            cron::confirm_payments(&self.dao, &self.get_name(), network_key).await;
        }
        log::trace!("ERC-20 confirmation job complete.");
        drop(guard); // Explicit drop to tell Rust that guard is not unused variable
    }

    async fn send_out_payments(&self) {
        let guard = match self.sendout_lock.try_lock() {
            None => {
                log::trace!("ERC-20 send-out job in progress.");
                return;
            }
            Some(guard) => guard,
        };

        log::trace!("Running ERC-20 send-out job...");
        'outer: for network_key in self.get_networks().keys() {
            let network = Network::from_str(network_key).unwrap();
            // Process payment rows
            let accounts = self.active_accounts.borrow().list_accounts();
            for node_id in accounts {
                if let Err(e) =
                    cron::process_payments_for_account(&self.dao, &node_id, network).await
                {
                    log::error!(
                        "Cron: processing payment for account [{}] failed with error: {}",
                        node_id,
                        e
                    );
                    continue 'outer;
                };
            }
            // Process transaction rows
            cron::process_transactions(&self.dao, network).await;
        }
        log::trace!("ERC-20 send-out job complete.");

        drop(guard); // Explicit drop to tell Rust that guard is not unused variable
    }

    fn sendout_interval(&self) -> std::time::Duration {
        *TX_SENDOUT_INTERVAL
    }

    fn confirmation_interval(&self) -> std::time::Duration {
        *TX_CONFIRMATION_INTERVAL
    }
}