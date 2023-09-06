/*
    Erc20Driver to handle payments on the erc20 network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use erc20_payment_lib::db::ops::insert_token_transfer;
use erc20_payment_lib::runtime::PaymentRuntime;
use erc20_payment_lib::transaction::create_token_transfer;
use ethereum_types::H160;
use num_bigint::BigInt;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

// Workspace uses
use ya_payment_driver::{
    account::{Accounts, AccountsRc},
    bus,
    cron::PaymentDriverCron,
    dao::DbExecutor,
    driver::{
        async_trait, BigDecimal, IdentityError, IdentityEvent, Network as NetworkConfig,
        PaymentDriver,
    },
    model::*,
};

// Local uses
use crate::{erc20::utils::big_dec_to_u256, network::platform_to_currency};
use crate::{network::SUPPORTED_NETWORKS, DRIVER_NAME, RINKEBY_NETWORK};

mod api;
mod cli;

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
    payment_runtime: PaymentRuntime,
}

impl Erc20NextDriver {
    pub fn new(pr: PaymentRuntime) -> Self {
        Self {
            active_accounts: Accounts::new_rc(),
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

    async fn do_transfer(
        &self,
        sender: &str,
        to: &str,
        amount: &BigDecimal,
        network: &str,
    ) -> Result<String, GenericError> {
        self.is_account_active(sender)?;
        let sender = H160::from_str(sender)
            .map_err(|err| GenericError::new(format!("Error when parsing sender {err:?}")))?;
        let receiver = H160::from_str(to)
            .map_err(|err| GenericError::new(format!("Error when parsing receiver {err:?}")))?;
        let amount = big_dec_to_u256(amount)?;

        let chain_id = self
            .payment_runtime
            .setup
            .chain_setup
            .values()
            .find(|chain_setup| &chain_setup.network == network)
            .ok_or_else(|| GenericError::new(format!("Unsupported network: {}", network)))?
            .chain_id;

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

        Ok(payment_id)
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
        let platform = msg.platform();
        let network = platform.split("-").nth(1).ok_or(GenericError::new(format!(
            "Malformed platform string: {}",
            msg.platform()
        )))?;

        let address_str = msg.address();
        let address = H160::from_str(&address_str).map_err(|e| {
            GenericError::new(format!(
                "{} isn't a valid H160 address: {}",
                address_str,
                e.to_string()
            ))
        })?;

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
        _db: DbExecutor,
        _caller: String,
        msg: GetAccountGasBalance,
    ) -> Result<Option<GasDetails>, GenericError> {
        let platform = msg.platform();
        let network = platform.split("-").nth(1).ok_or(GenericError::new(format!(
            "Malformed platform string: {}",
            msg.platform()
        )))?;

        let address_str = msg.address();
        let address = H160::from_str(&address_str).map_err(|e| {
            GenericError::new(format!(
                "{} isn't a valid H160 address: {}",
                address_str,
                e.to_string()
            ))
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
        log::info!("FUND = Not Implemented: {:?}", msg);
        Ok("NOT_IMPLEMENTED".to_string())
    }

    async fn transfer(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: Transfer,
    ) -> Result<String, GenericError> {
        let network = msg
            .network
            .ok_or(GenericError::new("Network not specified".to_string()))?;

        self.do_transfer(&msg.sender, &msg.to, &msg.amount, &network)
            .await
    }

    async fn schedule_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError> {
        let platform = msg.platform();
        let network = platform.split("-").nth(1).ok_or(GenericError::new(format!(
            "Malformed platform string: {}",
            msg.platform()
        )))?;

        self.do_transfer(&msg.sender(), &msg.recipient(), &msg.amount(), &network)
            .await
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
        // no-op, erc20_payment_lib driver doesn't expose clean shutdown interface yet
        Ok(())
    }
}

#[async_trait(?Send)]
impl PaymentDriverCron for Erc20NextDriver {
    async fn confirm_payments(&self) {
        // no-op, handled by erc20_payment_lib internally
    }

    async fn send_out_payments(&self) {
        // no-op, handled by erc20_payment_lib internally
    }

    fn sendout_interval(&self) -> std::time::Duration {
        *TX_SENDOUT_INTERVAL
    }

    fn confirmation_interval(&self) -> std::time::Duration {
        *TX_CONFIRMATION_INTERVAL
    }
}
