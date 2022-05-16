/*
    Erc20Driver to handle payments on the erc20 network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use log;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Notify;

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
use crate::{dao::Erc20Dao, network::SUPPORTED_NETWORKS, DRIVER_NAME, RINKEBY_NETWORK};

mod api;
mod cli;
mod cron;

lazy_static::lazy_static! {
    static ref TX_SENDOUT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(
            std::env::var("ERC20_SENDOUT_INTERVAL_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(120),
        );

    static ref TX_CONFIRMATION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(
            std::env::var("ERC20_CONFIRMATION_INTERVAL_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(10),
        );
}

pub struct Erc20Driver {
    active_accounts: AccountsRc,
    dao: Erc20Dao,
    notify: Arc<Notify>, //can be Rc also, but Arc looks nicer
}

impl Erc20Driver {
    pub fn new(db: DbExecutor) -> Self {
        Self {
            active_accounts: Accounts::new_rc(),
            dao: Erc20Dao::new(db),
            notify: Arc::new(Notify::new()),
        }
    }

    pub async fn load_active_accounts(&self) {
        log::debug!("load_active_accounts");
        let mut accounts = self.active_accounts.borrow_mut();
        let unlocked_accounts = bus::list_unlocked_identities().await.unwrap();
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
impl PaymentDriver for Erc20Driver {
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
    async fn exit_fee(&self, _: ExitFee) -> Result<FeeResult, GenericError> {
        log::info!("EXIT_FEE = Not Implemented");
        Err(GenericError::new("EXIT_FEE = Not Implemented"))
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
        let res = cli::transfer(&self.dao, msg).await;
        if res.is_ok() {
            self.notify.notify();
        }
        res
    }

    async fn transfer_fee(&self, _msg: TransferFee) -> Result<FeeResult, GenericError> {
        log::info!("transfer_fee = Not Implemented");
        Err(GenericError::new("transfer_fee = Not Implemented"))
    }

    async fn schedule_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError> {
        log::debug!("schedule_payment: {:?}", msg);

        self.is_account_active(&msg.sender())?;
        api::schedule_payment(&self.dao, msg).await
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
        _msg: ShutDown,
    ) -> Result<(), GenericError> {
        //TODO - add proper shutdown code
        //self.send_out_payments().await;
        // HACK: Make sure that send-out job did complete. It might have just been running in another thread (cron). In such case .send_out_payments() would not block.
        //self.sendout_lock.lock().await;
        /*
        let timeout = Duration::from_std(msg.timeout)
            .map_err(|e| GenericError::new(format!("Invalid shutdown timeout: {}", e)))?;
        let deadline = Utc::now() + timeout - Duration::seconds(1);
        while {
            //self.confirm_payments().await; // Run it at least once
            Utc::now() < deadline && self.dao.has_unconfirmed_txs().await? // Stop if deadline passes or there are no more transactions to confirm
        } {
            tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
        }*/
        Ok(())
    }
}

impl Erc20Driver {
    async fn confirm_payments(&self) -> Result<bool, GenericError> {
        log::trace!("Running ERC-20 confirmation job...");
        for network_key in self.get_networks().keys() {
            if !cron::confirm_payments(&self.dao, &self.get_name(), network_key).await? {
                return Ok(false);
            }
        }
        log::trace!("ERC-20 confirmation job complete.");
        Ok(true)
    }

    async fn send_out_payments(&self) {
        log::trace!("Running ERC-20 send-out job...");
        'outer: for network_key in self.get_networks().keys() {
            let network = Network::from_str(&network_key).unwrap();
            // Process payment rows
            for node_id in self.active_accounts.borrow().list_accounts() {
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
    }
}

#[async_trait(?Send)]
impl PaymentDriverCron for Erc20Driver {
    async fn start_confirmation_job(self: Arc<Self>) {
        let driver = self.clone();
        loop {
            tokio::select! {
                val = driver.notify.notified() => {
                    log::debug!("Received notification {:?}", val);
                }
                val = tokio::time::delay_for(*TX_SENDOUT_INTERVAL) => {
                    log::debug!("Start payment driver cron loop {:?}", val);
                }
            }
            self.send_out_payments().await;
            loop {
                tokio::time::delay_for(*TX_CONFIRMATION_INTERVAL).await;
                let res = self.confirm_payments().await.unwrap_or_else(|err| {
                    log::error!("Error when trying to confirm payments {}", err);
                    false
                });
                if res {
                    break;
                }
            }
        }

        /*
        let driver1 = self.clone();
        Arbiter::spawn(async move {
            loop {
                tokio::select! {
                    val = driver1.notify.notified() => {
                        log::debug!("Received notification {:?}", val);
                    }
                    val = tokio::time::delay_for(*TX_SENDOUT_INTERVAL) => {
                        log::debug!("Start payment driver cron loop {:?}", val);
                    }
                }
                driver1.send_out_payments().await;
            }
        });
        let driver2 = self.clone();
        Arbiter::spawn(async move {
            loop {
                tokio::select! {
                    val = driver2.notify.notified() => {
                        log::debug!("Received notification {:?}", val);
                    }
                    val = tokio::time::delay_for(*TX_SENDOUT_INTERVAL) => {
                        log::debug!("Start payment driver cron loop {:?}", val);
                    }
                }
                driver2.confirm_payments().await;
            }
        });*/
    }
}
