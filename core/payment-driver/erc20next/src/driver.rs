use chrono::{DateTime, Duration, Utc};
/*
    Erc20Driver to handle payments on the erc20next network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use erc20_payment_lib::db::model::{TokenTransferDao, TxDao};
use erc20_payment_lib::runtime::{
    DriverEvent, DriverEventContent, PaymentRuntime, TransferType, VerifyTransactionResult,
};
use ethereum_types::H160;
use ethereum_types::U256;
use num_bigint::BigInt;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use tokio_util::task::LocalPoolHandle;
use uuid::Uuid;
use web3::types::H256;
use ya_client_model::payment::DriverStatusProperty;

// Workspace uses
use ya_payment_driver::{
    account::{Accounts, AccountsArc},
    bus,
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
        LocalPoolHandle::new(1).spawn_pinned(move || Self::payment_confirm_job(this_, recv));

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
            .transfer(
                network,
                sender,
                receiver,
                TransferType::Token,
                amount,
                &payment_id,
                deadline,
            )
            .await
            .map_err(|err| GenericError::new(format!("Error when inserting transfer {err:?}")))?;

        Ok(payment_id)
    }

    async fn payment_confirm_job(this: Arc<Self>, mut events: Receiver<DriverEvent>) {
        while let Some(event) = events.recv().await {
            if let DriverEventContent::TransferFinished(transfer_finished) = &event.content {
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
        }
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
        self.active_accounts.lock().await.handle_event(msg);
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
        _db: DbExecutor,
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
    ) -> Result<Vec<DriverStatusProperty>, DriverStatusError> {
        use erc20_payment_lib::runtime::StatusProperty as LibStatusProperty;

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
                    Some(DriverStatusProperty::CantSign {
                        driver: DRIVER_NAME.into(),
                        network,
                        address,
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
                LibStatusProperty::NoToken {
                    chain_id,
                    missing_token,
                } => {
                    let network = chain_id_to_net(chain_id);
                    network_filter(&network).then(|| DriverStatusProperty::InsufficientToken {
                        driver: DRIVER_NAME.into(),
                        network,
                        needed_token_est: missing_token.unwrap_or_default().to_string(),
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
