/*
    ZksyncDriver to handle payments on the zksync network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use chrono::{Duration, TimeZone, Utc};
use futures::lock::Mutex;
use lazy_static::lazy_static;
use num_bigint::BigInt;
use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use uuid::Uuid;

// Workspace uses
use ya_payment_driver::{
    account::{Accounts, AccountsRc},
    bus,
    cron::PaymentDriverCron,
    dao::DbExecutor,
    db::models::{Network as DbNetwork, PaymentEntity, TxType},
    driver::{async_trait, BigDecimal, IdentityError, IdentityEvent, Network, PaymentDriver},
    model::*,
    utils,
};
use ya_utils_futures::timeout::IntoTimeoutFuture;

// Local uses
use crate::{
    dao::ZksyncDao,
    network::{
        get_network_token, network_token_to_platform, platform_to_network_token, SUPPORTED_NETWORKS,
    },
    zksync::wallet,
    DEFAULT_NETWORK, DRIVER_NAME,
};

lazy_static! {
    static ref TX_SUMBIT_TIMEOUT: Duration = Duration::minutes(15);
    static ref MAX_ALLOCATION_SURCHARGE: BigDecimal =
        match env::var("MAX_ALLOCATION_SURCHARGE").map(|s| s.parse()) {
            Ok(Ok(x)) => x,
            _ => BigDecimal::from(200),
        };

    // Environment variable will be replaced by allocation parameter in PAY-82
    static ref TRANSACTIONS_PER_ALLOCATION: BigInt =
        match env::var("TRANSACTIONS_PER_ALLOCATION").map(|s| s.parse()) {
            Ok(Ok(x)) => x,
            _ => BigInt::from(10),
        };

    static ref TX_SENDOUT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(
            std::env::var("ZKSYNC_SENDOUT_INTERVAL_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(10),
        );

    static ref TX_CONFIRMATION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(
            std::env::var("ZKSYNC_CONFIRMATION_INTERVAL_SECS")
                .ok()
                .and_then(|x| x.parse().ok())
                .unwrap_or(5),
        );
}

pub struct ZksyncDriver {
    active_accounts: AccountsRc,
    dao: ZksyncDao,
    sendout_lock: Mutex<()>,
    confirmation_lock: Mutex<()>,
}

impl ZksyncDriver {
    pub fn new(db: DbExecutor) -> Self {
        Self {
            active_accounts: Accounts::new_rc(),
            dao: ZksyncDao::new(db),
            sendout_lock: Default::default(),
            confirmation_lock: Default::default(),
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

    fn is_account_active(&self, address: &str) -> bool {
        self.active_accounts
            .as_ref()
            .borrow()
            .get_node_id(address)
            .is_some()
    }

    async fn process_payments_for_account(&self, node_id: &str) {
        log::trace!("Processing payments for node_id={}", node_id);
        for network_key in self.get_networks().keys() {
            let network = DbNetwork::from_str(&network_key).unwrap();
            let payments: Vec<PaymentEntity> =
                self.dao.get_pending_payments(node_id, network).await;
            let mut nonce = 0;
            if !payments.is_empty() {
                log::info!(
                    "Processing payments. count={}, network={} node_id={}",
                    payments.len(),
                    network_key,
                    node_id
                );

                nonce = wallet::get_nonce(node_id, network).await;
                log::debug!("Payments: nonce={}, details={:?}", &nonce, payments);
            }
            for payment in payments {
                self.handle_payment(payment, &mut nonce).await;
            }
        }
    }

    async fn handle_payment(&self, payment: PaymentEntity, nonce: &mut u32) {
        let details = utils::db_to_payment_details(&payment);
        let tx_nonce = nonce.to_owned();

        match wallet::make_transfer(&details, tx_nonce, payment.network).await {
            Ok(tx_hash) => {
                let tx_id = self
                    .dao
                    .insert_transaction(&details, Utc::now(), payment.network)
                    .await;
                self.dao
                    .transaction_sent(&tx_id, &tx_hash, &payment.order_id)
                    .await;
                *nonce += 1;
            }
            Err(e) => {
                let deadline =
                    Utc.from_utc_datetime(&payment.payment_due_date) + *TX_SUMBIT_TIMEOUT;
                if Utc::now() > deadline {
                    log::error!("Failed to submit zkSync transaction. Retry deadline reached. details={:?} error={}", payment, e);
                    self.dao.payment_failed(&payment.order_id).await;
                } else {
                    log::warn!(
                        "Failed to submit zkSync transaction. Payment will be retried until {}. details={:?} error={}",
                        deadline, payment, e
                    );
                };
            }
        };
    }
}

#[async_trait(?Send)]
impl PaymentDriver for ZksyncDriver {
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
        let tx_hash = wallet::enter(msg).await?;

        Ok(format!(
            "Deposit transaction has been sent on Ethereum and soon funds should be available \
        on ZkSync network. You can check transaction status in the block explorer. \
        Tracking link: https://rinkeby.zkscan.io/explorer/transactions/{}",
            tx_hash
        ))
    }

    async fn exit(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: Exit,
    ) -> Result<String, GenericError> {
        if !self.is_account_active(&msg.sender()) {
            return Err(GenericError::new(
                "Cannot start withdrawal, account is not active",
            ));
        }

        let tx_hash = wallet::exit(&msg).await?;
        Ok(format!(
            "Withdrawal has been accepted by the zkSync operator. \
        It may take some time until the funds are available on Ethereum blockchain. \
        Tracking link: https://rinkeby.zkscan.io/explorer/transactions/{}",
            tx_hash
        ))
    }

    async fn get_account_balance(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: GetAccountBalance,
    ) -> Result<BigDecimal, GenericError> {
        log::debug!("get_account_balance: {:?}", msg);
        let (network, _) = platform_to_network_token(msg.platform())?;

        let balance = wallet::account_balance(&msg.address(), network).await?;

        log::debug!("get_account_balance - result: {}", &balance);
        Ok(balance)
    }

    fn get_name(&self) -> String {
        DRIVER_NAME.to_string()
    }

    fn get_default_network(&self) -> String {
        DEFAULT_NETWORK.to_string()
    }

    fn get_networks(&self) -> HashMap<String, Network> {
        SUPPORTED_NETWORKS.clone()
    }

    fn recv_init_required(&self) -> bool {
        false
    }

    async fn init(&self, _db: DbExecutor, _caller: String, msg: Init) -> Result<Ack, GenericError> {
        log::debug!("init: {:?}", msg);
        let address = msg.address().clone();
        let mode = msg.mode();

        // Ensure account is unlock before initialising send mode
        if mode.contains(AccountMode::SEND) && !self.is_account_active(&address) {
            return Err(GenericError::new("Can not init, account not active"));
        }

        wallet::init_wallet(&msg)
            .timeout(Some(180))
            .await
            .map_err(GenericError::new)??;

        let network = msg.network().unwrap_or(DEFAULT_NETWORK.to_string());
        let token = get_network_token(
            DbNetwork::from_str(&network).map_err(GenericError::new)?,
            msg.token(),
        );
        bus::register_account(self, &address, &network, &token, mode).await?;

        log::info!(
            "Initialised payment account. mode={:?}, address={}, driver={}, network={}, token={}",
            mode,
            &address,
            DRIVER_NAME,
            network,
            token
        );
        Ok(Ack {})
    }

    async fn fund(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: Fund,
    ) -> Result<String, GenericError> {
        let address = msg.address();
        let network = DbNetwork::from_str(&msg.network().unwrap_or(DEFAULT_NETWORK.to_string()))
            .map_err(GenericError::new)?;
        match network {
            DbNetwork::Rinkeby => {
                log::info!(
                    "Handling fund request. network={}, address={}",
                    &network,
                    &address
                );
                wallet::fund(&address, network)
                    .timeout(Some(15)) // Regular scenario =~ 5s
                    .await
                    .map_err(GenericError::new)??;
                Ok(format!(
                    "Received funds from the faucet. address={}",
                    &address
                ))
            }
            DbNetwork::Goerli => Ok(format!("Goerli network is not supported by this driver.")),
            DbNetwork::Mumbai => Ok(format!("Mumbai network is not supported by this driver.")),
            DbNetwork::Polygon => Ok(format!("Polygon network is not supported by this driver.")),
            DbNetwork::Mainnet => Ok(format!(
                r#"Using this driver is not recommended. Consider using the Polygon driver instead.
                
Your mainnet zkSync address is {}.
To be able to use zkSync driver please send some GLM tokens and optionally ETH for gas to this address.
"#,
                address
            )),
        }
    }

    async fn transfer(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: Transfer,
    ) -> Result<String, GenericError> {
        log::info!("TRANSFER = Not Implemented: {:?}", msg);
        Ok("NOT_IMPLEMENTED".to_string())
    }

    async fn schedule_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError> {
        log::debug!("schedule_payment: {:?}", msg);

        let sender = msg.sender().to_owned();
        if !self.is_account_active(&sender) {
            return Err(GenericError::new(
                "Can not schedule_payment, account not active",
            ));
        }

        let order_id = Uuid::new_v4().to_string();
        self.dao.insert_payment(&order_id, &msg).await?;
        Ok(order_id)
    }

    async fn verify_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: VerifyPayment,
    ) -> Result<PaymentDetails, GenericError> {
        log::debug!("verify_payment: {:?}", msg);
        let (network, _) = platform_to_network_token(msg.platform())?;
        let tx_hash = hex::encode(msg.confirmation().confirmation);
        log::info!("Verifying transaction: {}", tx_hash);
        wallet::verify_tx(&tx_hash, network).await
    }

    async fn validate_allocation(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: ValidateAllocation,
    ) -> Result<bool, GenericError> {
        let (network, _) = platform_to_network_token(msg.platform)?;
        let account_balance = wallet::account_balance(&msg.address, network).await?;
        let total_allocated_amount: BigDecimal = msg
            .existing_allocations
            .into_iter()
            .map(|allocation| allocation.remaining_amount)
            .sum();

        // NOTE: `wallet::get_tx_fee` accepts an _recipient_ address which is unknown at the moment
        // so the _sender_ address is provider. This might bias fee calculation, because transaction
        // to new account is little more expensive.
        let tx_fee_cost = wallet::get_tx_fee(&msg.address, network).await?;
        let total_txs_cost = tx_fee_cost * &*TRANSACTIONS_PER_ALLOCATION;
        let allocation_surcharge = (&*MAX_ALLOCATION_SURCHARGE).min(&total_txs_cost);

        log::info!(
            "Allocation validation: \
            allocating: {:.5}, \
            account_balance: {:.5}, \
            total_allocated_amount: {:.5}, \
            allocation_surcharge: {:.5} \
            ",
            msg.amount,
            account_balance,
            total_allocated_amount,
            allocation_surcharge,
        );
        Ok(msg.amount <= (account_balance - total_allocated_amount - allocation_surcharge))
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
            tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
        }
        Ok(())
    }
}

#[async_trait(?Send)]
impl PaymentDriverCron for ZksyncDriver {
    async fn confirm_payments(&self) {
        let guard = match self.confirmation_lock.try_lock() {
            None => {
                log::trace!("ZkSync confirmation job in progress.");
                return;
            }
            Some(guard) => guard,
        };
        log::trace!("Running zkSync confirmation job...");

        for network_key in self.get_networks().keys() {
            let network =
                match DbNetwork::from_str(&network_key) {
                    Ok(n) => n,
                    Err(e) => {
                        log::error!(
                        "Failed to parse network, skipping confirmation job. key={}, error={:?}",
                        network_key, e);
                        continue;
                    }
                };
            let txs = self.dao.get_unconfirmed_txs(network).await;
            log::trace!("confirm_payments network={} txs={:?}", &network_key, &txs);

            for tx in txs {
                log::trace!("checking tx {:?}", &tx);
                let tx_hash = match &tx.tmp_onchain_txs {
                    None => continue,
                    Some(tx_hash) => tx_hash,
                };

                log::debug!(
                    "Checking if tx was a success. network={}, hash={}",
                    &network,
                    &tx_hash
                );
                let tx_success = match wallet::check_tx(&tx_hash, network).await {
                    None => continue, // Check_tx returns None when the result is unknown
                    Some(tx_success) => tx_success,
                };

                let payments = self.dao.transaction_confirmed(&tx.tx_id).await;

                // Faucet can stop here IF the tx was a success.
                if tx.tx_type == TxType::Faucet as i32 && tx_success.is_ok() {
                    log::debug!("Faucet tx confirmed, exit early. hash={}", &tx_hash);
                    continue;
                }

                let order_ids: Vec<String> = payments
                    .iter()
                    .map(|payment| payment.order_id.clone())
                    .collect();

                if let Err(err) = tx_success {
                    // In case of invalid nonce error we can retry sending transaction.
                    // Reset payment and transaction state to 'not sent', so cron job will pickup
                    // transaction again.
                    if err.contains("Nonce mismatch") {
                        log::warn!(
                            "Scheduling retry for tx {:?} because of nonce mismatch. ZkSync error: {}",
                            tx,
                            err
                        );

                        for order_id in order_ids.iter() {
                            self.dao.retry_payment(order_id).await;
                        }
                    } else {
                        log::error!(
                            "ZkSync transaction verification failed. tx_details={:?} error={}",
                            tx,
                            err
                        );

                        for order_id in order_ids.iter() {
                            self.dao.payment_failed(order_id).await;
                        }
                    }

                    self.dao
                        .transaction_failed(&tx.tx_id, "Unknown error")
                        .await;
                    return;
                }

                // TODO: Add token support
                let platform = network_token_to_platform(Some(network), None).unwrap(); // TODO: Catch error?
                let details = match wallet::verify_tx(&tx_hash, network).await {
                    Ok(a) => a,
                    Err(e) => {
                        log::warn!("Failed to get transaction details from zksync, creating bespoke details. Error={}", e);

                        //Create bespoke payment details:
                        // - Sender + receiver are the same
                        // - Date is always now
                        // - Amount needs to be updated to total of all PaymentEntity's
                        let first_payment: PaymentEntity =
                            match self.dao.get_first_payment(&tx_hash).await {
                                Some(p) => p,
                                None => continue,
                            };
                        let mut details = utils::db_to_payment_details(&first_payment);
                        details.amount = payments
                            .into_iter()
                            .map(|payment| utils::db_amount_to_big_dec(payment.amount.clone()))
                            .sum::<BigDecimal>();
                        details
                    }
                };
                if tx.tx_type == TxType::Transfer as i32 {
                    let tx_hash = hex::decode(&tx_hash).unwrap();

                    if let Err(e) = bus::notify_payment(
                        &self.get_name(),
                        &platform,
                        order_ids,
                        &details,
                        tx_hash,
                    )
                    .await
                    {
                        log::error!("{}", e)
                    };
                }
            }
        }
        log::trace!("ZkSync confirmation job complete.");
        drop(guard); // Explicit drop to tell Rust that guard is not unused variable
    }

    async fn send_out_payments(&self) {
        let guard = match self.sendout_lock.try_lock() {
            None => {
                log::trace!("ZkSync send-out job in progress.");
                return;
            }
            Some(guard) => guard,
        };
        log::trace!("Running zkSync send-out job...");
        for node_id in self.active_accounts.borrow().list_accounts() {
            self.process_payments_for_account(&node_id).await;
        }
        log::trace!("ZkSync send-out job complete.");
        drop(guard); // Explicit drop to tell Rust that guard is not unused variable
    }

    fn sendout_interval(&self) -> std::time::Duration {
        *TX_SENDOUT_INTERVAL
    }

    fn confirmation_interval(&self) -> std::time::Duration {
        *TX_CONFIRMATION_INTERVAL
    }
}
