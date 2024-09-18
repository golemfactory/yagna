use crate::api::allocations::{forced_release_allocation, release_allocation_after};
use crate::dao::{
    ActivityDao, AgreementDao, AllocationDao, AllocationStatus, OrderDao, PaymentDao, SyncNotifsDao,
};
use crate::error::processor::{
    AccountNotRegistered, GetStatusError, NotifyPaymentError, OrderValidationError,
    SchedulePaymentError, ValidateAllocationError, VerifyPaymentError,
};
use crate::models::order::ReadObj as DbOrder;
use crate::payment_sync::SYNC_NOTIFS_NOTIFY;
use crate::timeout_lock::{MutexTimeoutExt, RwLockTimeoutExt};

use actix_web::web::Data;
use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};
use futures::{FutureExt, TryFutureExt};
use metrics::counter;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};

use ya_client_model::payment::allocation::Deposit;
use ya_client_model::payment::{
    Account, ActivityPayment, AgreementPayment, DriverDetails, Network, Payment,
};
use ya_core_model::driver::{
    self, driver_bus_id, AccountMode, DriverReleaseDeposit, GetAccountBalanceResult,
    GetRpcEndpointsResult, PaymentConfirmation, PaymentDetails, ShutDown, ValidateAllocation,
    ValidateAllocationResult,
};
use ya_core_model::payment::local::{
    GenericError, GetAccountsError, GetDriversError, NotifyPayment, RegisterAccount,
    RegisterAccountError, RegisterDriver, RegisterDriverError, ReleaseDeposit, SchedulePayment,
    UnregisterAccount, UnregisterAccountError, UnregisterDriver, UnregisterDriverError,
};
use ya_core_model::payment::public::{SendPayment, SendSignedPayment, BUS_ID};
use ya_core_model::NodeId;
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_persistence::types::Role;
use ya_service_bus::typed::Endpoint;
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

fn driver_endpoint(driver: &str) -> Endpoint {
    bus::service(driver_bus_id(driver))
}

fn validate_orders(
    orders: &[DbOrder],
    platform: &str,
    payer_addr: &str,
    payee_addr: &str,
    amount: &BigDecimal,
) -> Result<(), OrderValidationError> {
    if orders.is_empty() {
        return Err(OrderValidationError::new(
            "orders not found in the database",
        ));
    }

    let mut total_amount = BigDecimal::zero();
    for order in orders.iter() {
        if order.amount.0 == BigDecimal::zero() {
            return OrderValidationError::zero_amount(order);
        }
        if order.payment_platform != platform {
            return OrderValidationError::platform(order, platform);
        }
        if order.payer_addr != payer_addr {
            return OrderValidationError::payer_addr(order, payer_addr);
        }
        if order.payee_addr != payee_addr {
            return OrderValidationError::payee_addr(order, payee_addr);
        }

        total_amount += &order.amount.0;
    }

    if &total_amount > amount {
        return OrderValidationError::amount(&total_amount, amount);
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct AccountDetails {
    pub driver: String,
    pub network: String,
    pub token: String,
    pub mode: AccountMode,
}

#[derive(Clone, Default)]
struct DriverRegistry {
    accounts: HashMap<(String, String), AccountDetails>,
    // (platform, address) -> details
    drivers: HashMap<String, DriverDetails>,
    // driver_name -> details
    platforms: HashMap<String, HashMap<String, bool>>, // platform -> (driver_name -> recv_init_required)
}

impl DriverRegistry {
    pub fn register_driver(&mut self, msg: RegisterDriver) -> Result<(), RegisterDriverError> {
        let RegisterDriver {
            driver_name,
            details,
        } = msg;
        log::trace!(
            "register_driver: driver_name={} details={:?}",
            driver_name,
            details
        );

        if !details.networks.contains_key(&details.default_network) {
            return Err(RegisterDriverError::InvalidDefaultNetwork(
                details.default_network,
            ));
        }
        for (network_name, network) in details.networks.iter() {
            if !network.tokens.contains_key(&network.default_token) {
                return Err(RegisterDriverError::InvalidDefaultToken(
                    network.default_token.clone(),
                    network_name.to_string(),
                ));
            }
            for (token, platform) in network.tokens.iter() {
                self.platforms
                    .entry(platform.clone())
                    .or_default()
                    .insert(driver_name.clone(), details.recv_init_required);
            }
        }
        self.drivers.insert(driver_name, details);
        Ok(())
    }

    pub fn unregister_driver(&mut self, msg: UnregisterDriver) {
        let driver_name = msg.0;
        let details = self.drivers.remove(&driver_name);
        if let Some(details) = details {
            for (network_name, network) in details.networks.iter() {
                for (token, platform) in network.tokens.iter() {
                    self.platforms
                        .entry(platform.clone())
                        .or_default()
                        .remove(&driver_name);
                }
            }
        }
        self.accounts
            .retain(|_, details| details.driver != driver_name);
    }

    pub fn register_account(&mut self, msg: RegisterAccount) -> Result<(), RegisterAccountError> {
        let driver_details = match self.drivers.get(&msg.driver) {
            None => return Err(RegisterAccountError::DriverNotRegistered(msg.driver)),
            Some(details) => details,
        };
        let network = match driver_details.networks.get(&msg.network) {
            None => {
                return Err(RegisterAccountError::UnsupportedNetwork(
                    msg.network,
                    msg.driver,
                ));
            }
            Some(network) => network,
        };
        let platform = match network.tokens.get(&msg.token) {
            None => {
                return Err(RegisterAccountError::UnsupportedToken(
                    msg.token,
                    msg.network,
                    msg.driver,
                ));
            }
            Some(platform) => platform.clone(),
        };

        match self.accounts.entry((platform, msg.address.clone())) {
            Entry::Occupied(mut entry) => {
                let details = entry.get_mut();
                if details.driver != msg.driver {
                    return Err(RegisterAccountError::AlreadyRegistered(
                        msg.address,
                        details.driver.to_string(),
                    ));
                }
                details.mode |= msg.mode;
            }
            Entry::Vacant(entry) => {
                entry.insert(AccountDetails {
                    driver: msg.driver,
                    network: msg.network,
                    token: msg.token,
                    mode: msg.mode,
                });
            }
        };
        Ok(())
    }

    pub fn unregister_account(&mut self, msg: UnregisterAccount) {
        self.accounts.remove(&(msg.platform, msg.address));
    }

    pub fn get_accounts(&self) -> Vec<Account> {
        self.accounts
            .iter()
            .map(|((platform, address), details)| Account {
                platform: platform.clone(),
                address: address.clone(),
                driver: details.driver.clone(),
                network: details.network.clone(),
                token: details.token.clone(),
                send: details.mode.contains(AccountMode::SEND),
                receive: details.mode.contains(AccountMode::RECV),
            })
            .collect()
    }

    pub fn get_driver(&self, driver: &str) -> Result<&DriverDetails, RegisterAccountError> {
        match self.drivers.get(driver) {
            None => Err(RegisterAccountError::DriverNotRegistered(driver.into())),
            Some(details) => Ok(details),
        }
    }

    pub fn get_drivers(&self) -> HashMap<String, DriverDetails> {
        self.drivers.clone()
    }

    pub fn get_network(
        &self,
        driver: String,
        network: Option<String>,
    ) -> Result<(String, Network), RegisterAccountError> {
        let driver_details = self.get_driver(&driver)?;
        // If network is not specified, use default network
        let network_name = network.unwrap_or_else(|| driver_details.default_network.to_owned());
        match driver_details.networks.get(&network_name) {
            None => Err(RegisterAccountError::UnsupportedNetwork(
                network_name,
                driver,
            )),
            Some(network_details) => Ok((network_name, network_details.clone())),
        }
    }

    pub fn get_platform(
        &self,
        driver: String,
        network: Option<String>,
        token: Option<String>,
    ) -> Result<String, RegisterAccountError> {
        let (network_name, network_details) = self.get_network(driver.clone(), network)?;
        let token = token.unwrap_or_else(|| network_details.default_token.to_owned());
        match network_details.tokens.get(&token) {
            None => Err(RegisterAccountError::UnsupportedToken(
                token,
                network_name,
                driver,
            )),
            Some(platform) => Ok(platform.into()),
        }
    }

    pub fn driver(
        &self,
        platform: &str,
        address: &str,
        mode: AccountMode,
    ) -> Result<String, AccountNotRegistered> {
        if let Some(details) = self
            .accounts
            .get(&(platform.to_owned(), address.to_owned()))
        {
            if details.mode.contains(mode) {
                return Ok(details.driver.clone());
            }
        }

        // If it's recv-only mode or no-mode (i.e. checking status) we can use any driver that
        // supports the given platform and doesn't require init for receiving.
        if !mode.contains(AccountMode::SEND) {
            if let Some(drivers) = self.platforms.get(platform) {
                for (driver, recv_init_required) in drivers.iter() {
                    if !*recv_init_required {
                        return Ok(driver.clone());
                    }
                }
            }
        }

        Err(AccountNotRegistered::new(platform, address, mode))
    }

    pub fn iter_drivers(&self) -> impl Iterator<Item = &String> {
        self.drivers.keys()
    }
}

const DB_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const REGISTRY_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

pub struct PaymentProcessor {
    db_executor: Arc<Mutex<DbExecutor>>,
    registry: RwLock<DriverRegistry>,
    in_shutdown: AtomicBool,
}

#[derive(Debug, PartialEq, Error)]
enum PaymentSendToGsbError {
    #[error("payment Send to Gsb failed")]
    Failed,
    #[error("payment Send to Gsb is not supported")]
    NotSupported,
    #[error("payment Send to Gsb has been rejected")]
    Rejected,
}

impl PaymentProcessor {
    pub fn new(db_executor: DbExecutor) -> Self {
        Self {
            db_executor: Arc::new(Mutex::new(db_executor)),
            registry: Default::default(),
            in_shutdown: AtomicBool::new(false),
        }
    }

    pub async fn register_driver(&self, msg: RegisterDriver) -> Result<(), RegisterDriverError> {
        self.registry
            .timeout_write(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| RegisterDriverError::InternalTimeout)?
            .register_driver(msg)
    }

    pub async fn unregister_driver(
        &self,
        msg: UnregisterDriver,
    ) -> Result<(), UnregisterDriverError> {
        self.registry
            .timeout_write(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| UnregisterDriverError::InternalTimeout)?
            .unregister_driver(msg);

        Ok(())
    }

    pub async fn register_account(&self, msg: RegisterAccount) -> Result<(), RegisterAccountError> {
        self.registry
            .timeout_write(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| RegisterAccountError::InternalTimeout)?
            .register_account(msg)
    }

    pub async fn unregister_account(
        &self,
        msg: UnregisterAccount,
    ) -> Result<(), UnregisterAccountError> {
        self.registry
            .timeout_write(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| UnregisterAccountError::InternalTimeout)?
            .unregister_account(msg);
        Ok(())
    }

    pub async fn get_accounts(&self) -> Result<Vec<Account>, GetAccountsError> {
        self.registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map(|registry| registry.get_accounts())
            .map_err(|_| GetAccountsError::InternalTimeout)
    }

    pub async fn get_drivers(&self) -> Result<HashMap<String, DriverDetails>, GetDriversError> {
        self.registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map(|registry| registry.get_drivers())
            .map_err(|_| GetDriversError::InternalTimeout)
    }

    pub async fn get_network(
        &self,
        driver: String,
        network: Option<String>,
    ) -> Result<(String, Network), RegisterAccountError> {
        self.registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| RegisterAccountError::InternalTimeout)?
            .get_network(driver, network)
    }

    pub async fn get_platform(
        &self,
        driver: String,
        network: Option<String>,
        token: Option<String>,
    ) -> Result<String, RegisterAccountError> {
        self.registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| RegisterAccountError::InternalTimeout)?
            .get_platform(driver, network, token)
    }

    pub async fn notify_payment(&self, msg: NotifyPayment) -> Result<(), NotifyPaymentError> {
        let driver = msg.driver;
        let payment_platform = msg.platform;
        let payer_addr = msg.sender;
        let payee_addr = msg.recipient;

        if msg.order_ids.is_empty() {
            return Err(OrderValidationError::new("order_ids is empty").into());
        }

        let payer_id: NodeId;
        let payee_id: NodeId;
        let payment_id: String;

        let payment: Payment = {
            let db_executor = self.db_executor.timeout_lock(DB_LOCK_TIMEOUT).await?;

            let orders = db_executor
                .as_dao::<OrderDao>()
                .get_many(msg.order_ids, driver.clone())
                .await?;
            validate_orders(
                &orders,
                &payment_platform,
                &payer_addr,
                &payee_addr,
                &msg.amount,
            )?;

            let mut activity_payments = vec![];
            let mut agreement_payments = vec![];
            for order in orders.iter() {
                let amount = order.amount.clone().into();
                match (order.activity_id.clone(), order.agreement_id.clone()) {
                    (Some(activity_id), None) => activity_payments.push(ActivityPayment {
                        activity_id,
                        amount,
                        allocation_id: Some(order.allocation_id.clone()),
                    }),
                    (None, Some(agreement_id)) => agreement_payments.push(AgreementPayment {
                        agreement_id,
                        amount,
                        allocation_id: Some(order.allocation_id.clone()),
                    }),
                    _ => return NotifyPaymentError::invalid_order(order),
                }
            }

            // FIXME: This is a hack. Payment orders realized by a single transaction are not guaranteed
            //        to have the same payer and payee IDs. Fixing this requires a major redesign of the
            //        data model. Payments can no longer by assigned to a single payer and payee.
            payer_id = orders.get(0).unwrap().payer_id;
            payee_id = orders.get(0).unwrap().payee_id;

            let payment_dao: PaymentDao = db_executor.as_dao();

            payment_id = payment_dao
                .create_new(
                    payer_id,
                    payee_id,
                    payer_addr,
                    payee_addr,
                    payment_platform.clone(),
                    msg.amount.clone(),
                    msg.confirmation.confirmation,
                    activity_payments,
                    agreement_payments,
                )
                .await?;

            let signed_payment = payment_dao
                .get(payment_id.clone(), payer_id)
                .await?
                .unwrap();
            signed_payment.payload
        };

        let signature_canonical = driver_endpoint(&driver)
            .send(driver::SignPaymentCanonicalized(payment.clone()))
            .await??;
        let signature = driver_endpoint(&driver)
            .send(driver::SignPayment(payment.clone()))
            .await??;

        counter!("payment.amount.sent", ya_metrics::utils::cryptocurrency_to_u64(&msg.amount), "platform" => payment_platform);
        // This is unconditional because at this point the invoice *has been paid*.
        // Whether the provider was correctly notified of this fact is another matter.
        counter!("payment.invoices.requestor.paid", 1);
        let msg = SendPayment::new(payment.clone(), signature);
        let msg_with_bytes = SendSignedPayment::new(payment.clone(), signature_canonical);

        let db_executor = Arc::clone(&self.db_executor);

        tokio::task::spawn_local(
            async move {
                let send_result =
                    Self::send_to_gsb(payer_id, payee_id, msg_with_bytes.clone()).await;

                let mark_sent = match send_result {
                    Ok(_) => true,
                    // If sending SendPaymentWithBytes is not supported then use SendPayment as fallback.
                    Err(PaymentSendToGsbError::NotSupported) => {
                        match Self::send_to_gsb(payer_id, payee_id, msg).await {
                            Ok(_) => true,
                            Err(PaymentSendToGsbError::Rejected) => true,
                            Err(PaymentSendToGsbError::Failed) => false,
                            Err(PaymentSendToGsbError::NotSupported) => false,
                        }
                    }
                    Err(_) => false,
                };

                let db_executor = db_executor.timeout_lock(DB_LOCK_TIMEOUT).await?;

                let payment_dao: PaymentDao = db_executor.as_dao();
                let sync_dao: SyncNotifsDao = db_executor.as_dao();

                // Always add new type of signature. Compatibility is for older Provider nodes only.
                payment_dao
                    .add_signature(
                        payment_id.clone(),
                        msg_with_bytes.signature.clone(),
                        msg_with_bytes.signed_bytes.clone(),
                    )
                    .await?;

                if mark_sent {
                    payment_dao.mark_sent(payment_id).await?;
                } else {
                    sync_dao.upsert(payee_id).await?;
                    SYNC_NOTIFS_NOTIFY.notify_one();
                    log::debug!("Failed to call SendPayment on [{payee_id}]");
                }

                anyhow::Ok(())
            }
            .inspect_err(|e| log::error!("Notify payment task failed: {e}")),
        );

        Ok(())
    }

    async fn send_to_gsb<T: RpcMessage + Unpin>(
        payer_id: NodeId,
        payee_id: NodeId,
        msg: T,
    ) -> Result<(), PaymentSendToGsbError> {
        ya_net::from(payer_id)
            .to(payee_id)
            .service(BUS_ID)
            .call(msg)
            .map(|res| match res {
                Ok(Ok(_)) => Ok(()),
                Err(e) if e.to_string().contains("endpoint address not found") => {
                    Err(PaymentSendToGsbError::NotSupported)
                }
                Err(err) => {
                    log::error!("Error sending payment message to provider: {:?}", err);
                    Err(PaymentSendToGsbError::Failed)
                }
                Ok(Err(err)) => {
                    log::error!("Provider rejected payment: {:?}", err);
                    Err(PaymentSendToGsbError::Rejected)
                }
            })
            .await
    }

    pub async fn schedule_payment(&self, msg: SchedulePayment) -> Result<(), SchedulePaymentError> {
        if self.in_shutdown.load(Ordering::SeqCst) {
            return Err(SchedulePaymentError::Shutdown);
        }
        let amount = msg.amount.clone();
        if amount <= BigDecimal::zero() {
            return Err(SchedulePaymentError::InvalidInput(format!(
                "Can not schedule payment with <=0 amount: {}",
                &amount
            )));
        }

        let allocation_status = self
            .db_executor
            .timeout_lock(DB_LOCK_TIMEOUT)
            .await?
            .as_dao::<AllocationDao>()
            .get(msg.allocation_id.clone(), msg.payer_id)
            .await?;
        let deposit_id = if let AllocationStatus::Active(allocation) = allocation_status {
            allocation.deposit
        } else {
            None
        };

        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await?
            .driver(&msg.payment_platform, &msg.payer_addr, AccountMode::SEND)?;

        let order_id = driver_endpoint(&driver)
            .send(driver::SchedulePayment::new(
                amount,
                msg.payer_addr.clone(),
                msg.payee_addr.clone(),
                msg.payment_platform.clone(),
                deposit_id,
                msg.due_date,
            ))
            .await??;

        self.db_executor
            .timeout_lock(DB_LOCK_TIMEOUT)
            .await?
            .as_dao::<OrderDao>()
            .create(msg, order_id, driver)
            .await?;

        Ok(())
    }

    pub async fn verify_payment(
        &self,
        payment: Payment,
        signature: Vec<u8>,
        canonical: Option<Vec<u8>>,
    ) -> Result<(), VerifyPaymentError> {
        // TODO: Split this into smaller functions
        let platform = payment.payment_platform.clone();
        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await?
            .driver(
                &payment.payment_platform,
                &payment.payee_addr,
                AccountMode::RECV,
            )?;

        if !driver_endpoint(&driver)
            .send(driver::VerifySignature::new(
                payment.clone(),
                signature.clone(),
                canonical.clone(),
            ))
            .await??
        {
            return Err(VerifyPaymentError::InvalidSignature);
        }

        let confirmation = match base64::decode(&payment.details) {
            Ok(confirmation) => PaymentConfirmation { confirmation },
            Err(e) => return Err(VerifyPaymentError::ConfirmationEncoding),
        };
        let details: PaymentDetails = driver_endpoint(&driver)
            .send(driver::VerifyPayment::new(
                confirmation.clone(),
                platform.clone(),
                payment.clone(),
            ))
            .await??;

        // Verify if amount declared in message matches actual amount transferred on blockchain
        if details.amount < payment.amount {
            return VerifyPaymentError::amount(&details.amount, &payment.amount);
        }

        // Verify if payment shares for agreements and activities sum up to the total amount
        let agreement_sum = payment.agreement_payments.iter().map(|p| &p.amount).sum();
        let activity_sum = payment.activity_payments.iter().map(|p| &p.amount).sum();
        if details.amount < (&agreement_sum + &activity_sum) {
            return VerifyPaymentError::shares(&details.amount, &agreement_sum, &activity_sum);
        }

        let payee_id = payment.payee_id;
        let payer_id = payment.payer_id;
        let payee_addr = &payment.payee_addr;
        let payer_addr = &payment.payer_addr;

        // Verify recipient address
        if &details.recipient != payee_addr {
            return VerifyPaymentError::recipient(payee_addr, &details.recipient);
        }
        if &details.sender != payer_addr {
            return VerifyPaymentError::sender(payer_addr, &details.sender);
        }

        {
            let db_executor = self.db_executor.timeout_lock(DB_LOCK_TIMEOUT).await?;

            // Verify agreement payments
            let agreement_dao: AgreementDao = db_executor.as_dao();
            for agreement_payment in payment.agreement_payments.iter() {
                let agreement_id = &agreement_payment.agreement_id;
                let agreement = agreement_dao.get(agreement_id.clone(), payee_id).await?;
                if agreement_payment.amount == BigDecimal::zero() {
                    return VerifyPaymentError::agreement_zero_amount(agreement_id);
                }
                match agreement {
                    None => return VerifyPaymentError::agreement_not_found(agreement_id),
                    Some(agreement) if &agreement.payee_addr != payee_addr => {
                        return VerifyPaymentError::agreement_payee(&agreement, payee_addr);
                    }
                    Some(agreement) if &agreement.payer_addr != payer_addr => {
                        return VerifyPaymentError::agreement_payer(&agreement, payer_addr);
                    }
                    Some(agreement) if agreement.payment_platform != payment.payment_platform => {
                        return VerifyPaymentError::agreement_platform(
                            &agreement,
                            &payment.payment_platform,
                        );
                    }
                    _ => (),
                }
            }

            // Verify activity payments
            let activity_dao: ActivityDao = db_executor.as_dao();
            for activity_payment in payment.activity_payments.iter() {
                let activity_id = &activity_payment.activity_id;
                if activity_payment.amount == BigDecimal::zero() {
                    return VerifyPaymentError::activity_zero_amount(activity_id);
                }
                let activity = activity_dao.get(activity_id.clone(), payee_id).await?;
                match activity {
                    None => return VerifyPaymentError::activity_not_found(activity_id),
                    Some(activity) if &activity.payee_addr != payee_addr => {
                        return VerifyPaymentError::activity_payee(&activity, payee_addr);
                    }
                    Some(activity) if &activity.payer_addr != payer_addr => {
                        return VerifyPaymentError::activity_payer(&activity, payer_addr);
                    }
                    _ => (),
                }
            }

            // Verify totals for all agreements and activities with the same confirmation
            let payment_dao: PaymentDao = db_executor.as_dao();
            let shared_payments = payment_dao
                .get_for_confirmation(confirmation.confirmation, Role::Provider)
                .await?;
            let other_payment_total = shared_payments
                .iter()
                .map(|payment| {
                    let agreement_total = payment
                        .agreement_payments
                        .iter()
                        .map(|ap| &ap.amount)
                        .sum::<BigDecimal>();

                    let activity_total = payment
                        .activity_payments
                        .iter()
                        .map(|ap| &ap.amount)
                        .sum::<BigDecimal>();

                    agreement_total + activity_total
                })
                .sum::<BigDecimal>();

            let all_payment_total = &other_payment_total + agreement_sum + activity_sum;
            if all_payment_total > details.amount {
                return VerifyPaymentError::overspending(&details.amount, &all_payment_total);
            }

            // Insert payment into database (this operation creates and updates all related entities)
            if canonical.is_none() {
                payment_dao
                    .insert_received(payment, payee_id, None, None)
                    .await?;
            } else {
                payment_dao
                    .insert_received(payment, payee_id, Some(signature), canonical)
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn get_status(
        &self,
        platform: String,
        address: String,
    ) -> Result<GetAccountBalanceResult, GetStatusError> {
        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await?
            .driver(&platform, &address, AccountMode::empty())?;
        let status = driver_endpoint(&driver)
            .send(driver::GetAccountBalance::new(address, platform))
            .await??;
        Ok(status)
    }

    pub async fn get_rpc_endpoints_info(
        &self,
        platform: String,
        address: String,
        network: Option<String>,
        verify: bool,
        resolve: bool,
        no_wait: bool,
    ) -> Result<GetRpcEndpointsResult, GetStatusError> {
        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await?
            .driver(&platform, &address, AccountMode::empty())?;
        let res = driver_endpoint(&driver)
            .send(driver::GetRpcEndpoints {
                network,
                verify,
                resolve,
                no_wait,
            })
            .await??;
        Ok(res)
    }

    pub async fn validate_allocation(
        &self,
        platform: String,
        address: String,
        amount: BigDecimal,
        timeout: Option<DateTime<Utc>>,
        deposit: Option<Deposit>,
        new_allocation: bool,
    ) -> Result<ValidateAllocationResult, ValidateAllocationError> {
        if self.in_shutdown.load(Ordering::SeqCst) {
            return Err(ValidateAllocationError::Shutdown);
        }

        if let Some(requested_timeout) = timeout {
            if requested_timeout < chrono::Utc::now() {
                return Ok(ValidateAllocationResult::TimeoutPassed { requested_timeout });
            }
        }

        let (active_allocations, past_allocations) = {
            let db = self.db_executor.timeout_lock(DB_LOCK_TIMEOUT).await?;
            let dao = db.as_dao::<AllocationDao>();

            let active = dao
                .get_for_address(platform.clone(), address.clone(), Some(false))
                .await?;
            let past = dao
                .get_for_address(platform.clone(), address.clone(), Some(true))
                .await?;

            (active, past)
        };

        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await?
            .driver(&platform, &address, AccountMode::empty())?;
        let msg = ValidateAllocation {
            address,
            platform,
            amount,
            timeout,
            deposit,
            active_allocations,
            past_allocations,
            new_allocation,
        };
        let result = driver_endpoint(&driver).send(msg).await??;
        Ok(result)
    }

    /// This function releases allocations.
    /// When `bool` is `true` all existing allocations are released immediately.
    /// For `false` each allocation timestamp is respected.
    pub async fn release_allocations(&self, force: bool) {
        // keep this lock alive for the entirety of this function for now
        let db_executor = match self.db_executor.timeout_lock(DB_LOCK_TIMEOUT).await {
            Ok(db) => db,
            Err(_) => {
                log::error!("Timed out waiting for db lock");
                return;
            }
        };

        let db = Data::new(db_executor.clone());
        let active_allocations = db
            .clone()
            .as_dao::<AllocationDao>()
            .get_filtered(None, None, None, None, None, Some(false))
            .await;

        if force {
            log::info!("Releasing all active allocations...");
        } else {
            log::info!("Releasing expired allocations...");
        }

        match active_allocations {
            Ok(allocations) => {
                if !allocations.is_empty() {
                    for allocation in allocations {
                        if force {
                            forced_release_allocation(db.clone(), allocation.allocation_id, None)
                                .await
                        } else {
                            release_allocation_after(
                                db.clone(),
                                allocation.allocation_id,
                                allocation.timeout,
                                None,
                            )
                            .await
                        }
                    }
                } else {
                    log::info!("No allocations found to be released.")
                }
            }
            Err(e) => {
                log::error!("Allocations release failed. Restart yagna to retry allocations release. Db error occurred: {}.", e);
            }
        }
    }

    pub async fn release_deposit(&self, msg: ReleaseDeposit) -> Result<(), GenericError> {
        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(GenericError::new)?
            .driver(&msg.platform, &msg.from, AccountMode::SEND)
            .map_err(GenericError::new)?;

        driver_endpoint(&driver)
            .send(DriverReleaseDeposit {
                platform: msg.platform,
                from: msg.from,
                deposit_contract: msg.deposit_contract,
                deposit_id: msg.deposit_id,
            })
            .await
            .map_err(GenericError::new)?
            .map_err(GenericError::new)?;

        Ok(())
    }

    pub async fn shut_down(
        &self,
        timeout: Duration,
    ) -> impl futures::Future<Output = ()> + 'static {
        self.in_shutdown.store(true, Ordering::SeqCst);

        let driver_shutdown_futures: Vec<_> = {
            let registry = self
                .registry
                .timeout_read(REGISTRY_LOCK_TIMEOUT)
                .await
                .expect("Can't initiate payment shutdown: registry lock timed out");

            registry
                .iter_drivers()
                .map(|driver| shut_down_driver(driver, timeout))
                .collect()
        };
        futures::future::join_all(driver_shutdown_futures).map(|_| ())
    }
}

fn shut_down_driver(
    driver: &str,
    timeout: Duration,
) -> impl futures::Future<Output = ()> + 'static {
    let driver = driver.to_string();
    let endpoint = driver_endpoint(&driver);
    let shutdown_msg = ShutDown::new(timeout);
    async move {
        log::info!("Shutting down driver '{}'... timeout={:?}", driver, timeout);
        match endpoint.call(shutdown_msg).await {
            Ok(Ok(_)) => log::info!("Driver '{}' shut down successfully.", driver),
            err => log::error!("Error shutting down driver '{}': {:?}", driver, err),
        }
    }
}
