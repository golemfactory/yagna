/*
    ZksyncDriver to handle payments on the zksync network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use chrono::Utc;
use std::collections::HashMap;
use std::str::FromStr;
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
use crate::{
    dao::ZksyncDao,
    network::{
        get_network_token, network_token_to_platform, platform_to_network_token, SUPPORTED_NETWORKS,
    },
    zksync::wallet,
    DEFAULT_NETWORK, DRIVER_NAME,
};

pub struct ZksyncDriver {
    active_accounts: AccountsRc,
    dao: ZksyncDao,
}

impl ZksyncDriver {
    pub fn new(db: DbExecutor) -> Self {
        Self {
            active_accounts: Accounts::new_rc(),
            dao: ZksyncDao::new(db),
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
        let tx_id = self.dao.insert_transaction(&details, Utc::now()).await;
        let tx_nonce = nonce.to_owned();

        match wallet::make_transfer(&details, tx_nonce, payment.network).await {
            Ok(tx_hash) => {
                self.dao
                    .transaction_success(&tx_id, &tx_hash, &payment.order_id)
                    .await;
                *nonce += 1;
            }
            Err(e) => {
                self.dao
                    .transaction_failed(&tx_id, &e, &payment.order_id)
                    .await;
                log::error!("NGNT transfer failed: {}", e);
                //return Err(e);
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
        let address = msg.address().clone();

        // TODO: payment_api fails to start due to provider account not unlocked
        // if !self.is_account_active(&address) {
        //     return Err(GenericError::new("Can not init, account not active"));
        // }

        wallet::init_wallet(&msg)
            .timeout(Some(180))
            .await
            .map_err(GenericError::new)??;

        let mode = msg.mode();
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
                wallet::fund(&address, network)
                    .timeout(Some(180))
                    .await
                    .map_err(GenericError::new)??;
                Ok(format!(
                    "Received funds from the faucet. address={}",
                    &address
                ))
            }
            DbNetwork::Mainnet => Ok(format!(
                "Your mainnet zksync address is {}. Send some GLM tokens to this address on zksync \
                to be able to use this driver.", address
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
                Some(a) => a,
            };
            // Check payments before to fetch network
            let first_payment: PaymentEntity = match self.dao.get_first_payment(&tx_hash).await {
                Some(p) => p,
                None => continue,
            };

            // Check_tx returns None when the result is unknown
            if let Some(result) = wallet::check_tx(&tx_hash, first_payment.network).await {
                let payments = self.dao.transaction_confirmed(&tx.tx_id, result).await;
                if !result {
                    log::warn!("Payment failed, will be re-tried.");
                    continue;
                }
                let order_ids = payments
                    .iter()
                    .map(|payment| payment.order_id.clone())
                    .collect();

                // TODO: Add token support
                let platform =
                    network_token_to_platform(Some(first_payment.network), None).unwrap(); // TODO: Catch error?
                let details = match wallet::verify_tx(&tx_hash, first_payment.network).await {
                    Ok(a) => a,
                    Err(e) => {
                        log::warn!("Failed to get transaction details from zksync, creating bespoke details. Error={}", e);

                        // Create bespoke payment details:
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
                    bus::notify_payment(&self.get_name(), &platform, order_ids, &details, tx_hash)
                        .await
                {
                    log::error!("{}", e)
                };
            }
        }
    }

    async fn process_payments(&self) {
        for node_id in self.active_accounts.borrow().list_accounts() {
            self.process_payments_for_account(&node_id).await;
        }
    }
}
