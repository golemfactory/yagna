/*
    Erc20Driver to handle payments on the erc20 network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use erc20_payment_lib::db::model::{TokenTransferDao, TxDao};
use erc20_payment_lib::runtime::{DriverEvent, DriverEventContent, PaymentRuntime, TransferType};
use ethereum_types::H160;
use ethereum_types::U256;
use num_bigint::BigInt;
use std::collections::HashMap;
use std::str::FromStr;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;
use uuid::Uuid;
use ya_client_model::payment::DriverStatusProperty;

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
use crate::erc20::utils::{big_dec_to_u256, u256_to_big_dec};
use crate::network::platform_to_currency;
use crate::{network::SUPPORTED_NETWORKS, DRIVER_NAME, RINKEBY_NETWORK};

mod api;
mod cli;

lazy_static::lazy_static! {
    static ref TX_SENDOUT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(
            std::env::var("ERC20NEXT_SENDOUT_INTERVAL_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(30),
        );

    static ref TX_CONFIRMATION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(
            std::env::var("ERC20NEXT_CONFIRMATION_INTERVAL_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(30),
        );
}

pub struct Erc20NextDriver {
    active_accounts: AccountsRc,
    payment_runtime: PaymentRuntime,
    events: Mutex<Receiver<DriverEvent>>,
}

impl Erc20NextDriver {
    pub fn new(payment_runtime: PaymentRuntime, events: Receiver<DriverEvent>) -> Self {
        Self {
            active_accounts: Accounts::new_rc(),
            payment_runtime,
            events: Mutex::new(events),
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

        let payment_id = Uuid::new_v4().to_simple().to_string();

        self.payment_runtime
            .transfer(
                network,
                sender,
                receiver,
                TransferType::Token,
                amount,
                &payment_id,
            )
            .await
            .map_err(|err| GenericError::new(format!("Error when inserting transfer {err:?}")))?;

        Ok(payment_id)
    }

    async fn _confirm_payments(
        &self,
        token_transfer: &TokenTransferDao,
        tx: &TxDao,
    ) -> Result<(), GenericError> {
        log::info!("Received event TransferFinished: {:#?}", token_transfer);

        let chain_id = token_transfer.chain_id;
        let network_name = &self
            .payment_runtime
            .setup
            .chain_setup
            .get(&chain_id)
            .ok_or(GenericError::new(format!(
                "Missing configuration for chain_id {chain_id}"
            )))?
            .network;

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
            return Err(GenericError::new(format!("Malformed token_transfer.token_amount: {}", token_transfer.token_amount)));
        };
        let Ok(tx_token_amount) = u256_to_big_dec(tx_token_amount) else {
            return Err(GenericError::new(format!("Cannot convert to big decimal tx_token_amount: {}", tx_token_amount)));
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

        log::info!("name = {}", &self.get_name());
        log::info!("platform = {}", platform);
        log::info!("order_id = {}", token_transfer.payment_id.as_ref().unwrap());
        log::info!("payment_details = {:#?}", payment_details);
        log::info!("confirmation = {:x?}", transaction_hash);

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
        _db: DbExecutor,
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
        log::debug!("schedule_payment: {:?}", msg);

        let platform = msg.platform();
        let network = platform.split('-').nth(1).ok_or(GenericError::new(format!(
            "Malformed platform string: {}",
            msg.platform()
        )))?;

        self.do_transfer(&msg.sender(), &msg.recipient(), &msg.amount(), network)
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
        db: DbExecutor,
        caller: String,
        msg: ValidateAllocation,
    ) -> Result<bool, GenericError> {
        log::info!("Validate_allocation: {:?}", msg);
        let account_balance = self
            .get_account_balance(
                db,
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
        _db: DbExecutor,
        _caller: String,
        msg: DriverStatus,
    ) -> Result<Vec<DriverStatusProperty>, GenericError> {
        use erc20_payment_lib::runtime::StatusProperty as LibStatusProperty;

        // Map chain-id to network
        let chain_id_to_net = |id: i64| {
            self.payment_runtime
                .setup
                .chain_setup
                .get(&id)
                .unwrap()
                .network
                .clone()
        };

        // check if network matches the filter
        let network_filter = |net_candidate: &str| {
            msg.network
                .as_ref()
                .map(|net| net == net_candidate)
                .unwrap_or(true)
        };

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
                LibStatusProperty::NoGas {
                    chain_id,
                    missing_gas,
                } => {
                    let network = chain_id_to_net(chain_id);
                    network_filter(&network).then(|| DriverStatusProperty::InsufficientGas {
                        driver: DRIVER_NAME.into(),
                        network,
                        needed_gas_est: missing_gas.unwrap_or_default().to_string(),
                    })
                }
            })
            .collect())
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
    fn sendout_interval(&self) -> std::time::Duration {
        *TX_SENDOUT_INTERVAL
    }

    fn confirmation_interval(&self) -> std::time::Duration {
        *TX_CONFIRMATION_INTERVAL
    }

    async fn send_out_payments(&self) {
        // no-op, handled by erc20_payment_lib internally
    }

    async fn confirm_payments(&self) {
        let mut events = self.events.lock().await;
        while let Ok(event) = events.try_recv() {
            if let DriverEventContent::TransferFinished(transfer_finished) = &event.content {
                match self
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
        }
    }
}
