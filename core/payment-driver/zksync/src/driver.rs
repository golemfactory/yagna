/*
    ZksyncDriver to handle payments on the zksync network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use chrono::{Duration, TimeZone, Utc};
use std::collections::HashMap;
use uuid::Uuid;

// Workspace uses
use ya_payment_driver::{
    account::{Accounts, AccountsRc},
    bus,
    cron::PaymentDriverCron,
    dao::DbExecutor,
    db::models::{Network as DbNetwork, PaymentEntity},
    driver::{async_trait, BigDecimal, IdentityError, IdentityEvent, Network, PaymentDriver},
    model::*,
    utils,
};
use ya_utils_futures::timeout::IntoTimeoutFuture;

// Local uses
use crate::{config::DriverConfig, dao::ZksyncDao, zksync::client::ZkSyncClient};

lazy_static! {
    static ref TX_SUMBIT_TIMEOUT: Duration = Duration::minutes(15);
}

pub struct ZksyncDriver {
    active_accounts: AccountsRc,
    dao: ZksyncDao,
    config: DriverConfig,
}

impl ZksyncDriver {
    pub fn new(db: DbExecutor, config: DriverConfig) -> Self {
        Self {
            active_accounts: Accounts::new_rc(),
            dao: ZksyncDao::new(db),
            config,
        }
    }

    fn client(&self, network: DbNetwork) -> ZkSyncClient {
        let config = self.config.networks.get(&network).unwrap();
        ZkSyncClient::new(network, config.to_owned())
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
        for network in self.config.networks.keys() {
            let network = network.to_owned();
            let payments: Vec<PaymentEntity> =
                self.dao.get_pending_payments(node_id, network).await;
            let mut nonce = 0;
            if !payments.is_empty() {
                log::info!(
                    "Processing payments. count={}, network={} node_id={}",
                    payments.len(),
                    network,
                    node_id
                );

                nonce = self.client(network).get_nonce(node_id).await;
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

        match self
            .client(payment.network)
            .make_transfer(&details, tx_nonce)
            .await
        {
            Ok(tx_hash) => {
                let tx_id = self.dao.insert_transaction(&details, Utc::now()).await;
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
        log::info!("ENTER = Not Implemented: {:?}", msg);
        Ok("NOT_IMPLEMENTED".to_string())
    }

    async fn exit(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: Exit,
    ) -> Result<String, GenericError> {
        if !self.is_account_active(&msg.sender) {
            return Err(GenericError::new(
                "Cannot start withdrawal, account is not active",
            ));
        }
        let network = self.config.resolve_network(msg.network.as_deref())?;
        let tx_hash = self
            .client(network)
            .exit(&msg.sender, msg.to.as_deref(), msg.amount)
            .await?;
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
        let (network, _) = self.config.platform_to_network_token(&msg.platform)?;

        let balance = self.client(network).account_balance(&msg.address).await?;

        log::debug!("get_account_balance - result: {}", &balance);
        Ok(balance)
    }

    fn get_name(&self) -> String {
        self.config.name.clone()
    }

    fn get_default_network(&self) -> String {
        self.config.default_network.to_string()
    }

    fn get_networks(&self) -> HashMap<String, Network> {
        self.config.supported_networks()
    }

    fn recv_init_required(&self) -> bool {
        false
    }

    async fn get_transaction_balance(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: GetTransactionBalance,
    ) -> Result<BigDecimal, GenericError> {
        log::debug!("get_transaction_balance: {:?}", msg);
        // TODO: Get real transaction balance
        Ok(BigDecimal::from(1_000_000_000_000_000_000u64))
    }

    async fn init(&self, _db: DbExecutor, _caller: String, msg: Init) -> Result<Ack, GenericError> {
        log::debug!("init: {:?}", msg);
        let address = msg.address;

        // TODO: payment_api fails to start due to provider account not unlocked
        // if !self.is_account_active(&address) {
        //     return Err(GenericError::new("Can not init, account not active"));
        // }

        let network = self.config.resolve_network(msg.network.as_deref())?;
        let token = self.config.get_network_token(network, msg.token);
        let mode = msg.mode;

        self.client(network)
            .init_wallet(&address, mode)
            .timeout(Some(180))
            .await
            .map_err(GenericError::new)??;

        bus::register_account(self, &address, &network.to_string(), &token, mode).await?;

        log::info!(
            "Initialised payment account. mode={:?}, address={}, driver={}, network={}, token={}",
            mode,
            &address,
            self.config.name,
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
        let address = msg.address;
        let network = self.config.resolve_network(msg.network.as_deref())?;
        match network {
            DbNetwork::Rinkeby => {
                self.client(network)
                    .fund(&address)
                    .timeout(Some(180))
                    .await
                    .map_err(GenericError::new)??;
                Ok(format!(
                    "Received funds from the faucet. address={}",
                    &address
                ))
            }
            DbNetwork::Mainnet => Ok(format!(
                r#"Your mainnet zkSync address is {}.

To fund your wallet and be able to pay for your activities on Golem head to
the https://chat.golem.network, join the #funding channel and type /terms
and follow instructions to request GLMs.

Mind that to be eligible you have to run your app at least once on testnet -
- we will verify if that is true so we can avoid people requesting "free GLMs"."#,
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

        if !self.is_account_active(&msg.sender) {
            return Err(GenericError::new(
                "Can not schedule_payment, account not active",
            ));
        }

        let order_id = Uuid::new_v4().to_string();
        let (network, _) = self.config.platform_to_network_token(&msg.platform)?;
        self.dao
            .insert_payment(
                order_id.clone(),
                msg.sender,
                msg.recipient,
                network,
                msg.amount,
                msg.due_date.naive_utc(),
            )
            .await?;
        Ok(order_id)
    }

    async fn verify_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: VerifyPayment,
    ) -> Result<PaymentDetails, GenericError> {
        log::debug!("verify_payment: {:?}", msg);
        let (network, _) = self.config.platform_to_network_token(&msg.platform)?;
        let tx_hash = hex::encode(msg.confirmation.confirmation);
        log::info!("Verifying transaction: {}", tx_hash);
        self.client(network).verify_tx(&tx_hash).await
    }

    async fn validate_allocation(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: ValidateAllocation,
    ) -> Result<bool, GenericError> {
        let (network, _) = self.config.platform_to_network_token(&msg.platform)?;
        let account_balance = self.client(network).account_balance(&msg.address).await?;
        let total_allocated_amount: BigDecimal = msg
            .existing_allocations
            .into_iter()
            .map(|allocation| allocation.remaining_amount)
            .sum();
        Ok(msg.amount <= (account_balance - total_allocated_amount))
    }
}

#[async_trait(?Send)]
impl PaymentDriverCron for ZksyncDriver {
    async fn confirm_payments(&self) {
        let txs = self.dao.get_unconfirmed_txs().await;
        log::trace!("confirm_payments {:?}", txs);

        for tx in txs {
            log::trace!("checking tx {:?}", &tx);
            let tx_hash = match &tx.tx_hash {
                None => continue,
                Some(tx_hash) => tx_hash,
            };
            // Check payments before to fetch network
            let first_payment: PaymentEntity = match self.dao.get_first_payment(&tx_hash).await {
                Some(p) => p,
                None => continue,
            };

            let tx_success = match self.client(first_payment.network).check_tx(&tx_hash).await {
                None => continue, // Check_tx returns None when the result is unknown
                Some(tx_success) => tx_success,
            };

            let payments = self.dao.transaction_confirmed(&tx.tx_id).await;
            let order_ids: Vec<String> = payments
                .iter()
                .map(|payment| payment.order_id.clone())
                .collect();

            if let Err(err) = tx_success {
                log::error!(
                    "ZkSync transaction verification failed. tx_details={:?} error={}",
                    tx,
                    err
                );
                self.dao.transaction_failed(&tx.tx_id).await;
                for order_id in order_ids.iter() {
                    self.dao.payment_failed(order_id).await;
                }
                return;
            }

            // TODO: Add token support
            let platform = self
                .config
                .network_token_to_platform(Some(first_payment.network), None)
                .unwrap(); // TODO: Catch error?
            let details = match self.client(first_payment.network).verify_tx(&tx_hash).await {
                Ok(a) => a,
                Err(e) => {
                    log::warn!("Failed to get transaction details from zksync, creating bespoke details. Error={}", e);

                    //Create bespoke payment details:
                    // - Sender + receiver are the same
                    // - Date is always now
                    // - Amount needs to be updated to total of all PaymentEntity's
                    let mut details = utils::db_to_payment_details(&first_payment);
                    details.amount = payments
                        .into_iter()
                        .map(|payment| utils::db_amount_to_big_dec(payment.amount.clone()))
                        .sum::<BigDecimal>();
                    details
                }
            };
            let tx_hash = hex::decode(&tx_hash).unwrap();

            if let Err(e) =
                bus::notify_payment(&self.get_name(), &platform, order_ids, &details, tx_hash).await
            {
                log::error!("{}", e)
            };
        }
    }

    async fn process_payments(&self) {
        for node_id in self.active_accounts.borrow().list_accounts() {
            self.process_payments_for_account(&node_id).await;
        }
    }
}
