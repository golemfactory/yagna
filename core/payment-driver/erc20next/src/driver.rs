/*
    Erc20Driver to handle payments on the erc20 network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
//use chrono::{Duration, Utc};
use erc20_payment_lib::runtime::PaymentRuntime;
use futures::lock::Mutex;
use std::collections::HashMap;
use std::ops::DerefMut;
use std::str::FromStr;
use web3::types::{U256, H160};
//use std::str::FromStr;

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
                .unwrap_or(30),
        );

    static ref TX_CONFIRMATION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(
            std::env::var("ERC20_CONFIRMATION_INTERVAL_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(30),
        );
}

pub struct Erc20Driver {
    active_accounts: AccountsRc,
    dao: Erc20Dao,
    sendout_lock: Mutex<()>,
    confirmation_lock: Mutex<()>,
    pub payment_runtime: PaymentRuntime,
}

impl Erc20Driver {
    pub fn new(db: DbExecutor, payment_runtime: PaymentRuntime) -> Self {
        Self {
            active_accounts: Accounts::new_rc(),
            dao: Erc20Dao::new(db),
            sendout_lock: Default::default(),
            confirmation_lock: Default::default(),
            payment_runtime: payment_runtime,
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
        cli::transfer(&self.dao, msg).await
    }

    async fn schedule_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError> {
        log::debug!("schedule_payment: {:?}", msg);
        use erc20_payment_lib::service::add_glm_request;

        self.is_account_active(&msg.sender())?;
        //self.payment_runtime.
        //api::schedule_payment(&self.dao, msg).await
        /*{
            let mut conn = self.payment_runtime.conn.lock().await;
            let payer_addr = H160::from_str(&msg.sender()).unwrap();
            let payee_addr = H160::from_str(&msg.recipient()).unwrap();
            let amount = msg.amount();
            let amount = U256::from_dec_str(&amount.to_string()).unwrap();
            add_glm_request(
                conn.deref_mut(),
                self.payment_runtime.setup.chain_setup.get(&4).unwrap(),
                amount,
                "",
                payer_addr,
                payee_addr

            ).await;
        }*/
        return Ok("".to_string());
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
        Ok(())
    }
}
