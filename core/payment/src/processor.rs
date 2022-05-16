use crate::api::allocations::{forced_release_allocation, release_allocation_after};
use crate::dao::{ActivityDao, AgreementDao, AllocationDao, BatchDao, OrderDao, PaymentDao};
use crate::error::processor::{
    AccountNotRegistered, GetStatusError, NotifyPaymentError, OrderValidationError,
    SchedulePaymentError, ValidateAllocationError, VerifyPaymentError,
};
use crate::models::batch::BatchPaymentObligation;
use crate::models::order::ReadObj as DbOrder;
use actix_web::{rt::Arbiter, web::Data};
use bigdecimal::{BigDecimal, Zero};
use chrono::Utc;
use futures::FutureExt;
use metrics::counter;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::Duration;

use ya_client_model::payment::{
    Account, ActivityPayment, AgreementPayment, DriverDetails, Network, Payment,
};
use ya_core_model::driver::{
    self, driver_bus_id, AccountMode,  BatchMode,GasDetails, PaymentConfirmation, PaymentDetails, ShutDown,
    ValidateAllocation,
};
use ya_core_model::payment::local::{
    NotifyPayment, RegisterAccount, RegisterAccountError, RegisterDriver, RegisterDriverError,
    SchedulePayment, UnregisterAccount, UnregisterDriver,
};
use ya_core_model::payment::public::{SendPayment, BUS_ID};
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::Endpoint;
use ya_service_bus::{typed as bus, RpcEndpoint};

fn driver_endpoint(driver: &str) -> Endpoint {
    bus::service(driver_bus_id(driver))
}

async fn validate_orders(
    orders: &Vec<DbOrder>,
    platform: &str,
    payer_addr: &str,
    payee_addr: &str,
    amount: &BigDecimal,
) -> Result<(), OrderValidationError> {
    if orders.is_empty() {
        return Err(OrderValidationError::new("orders not found in the database").into());
    }

    let mut total_amount = BigDecimal::zero();
    for order in orders.iter() {
        if &order.payment_platform != platform {
            return OrderValidationError::platform(order, platform);
        }
        if &order.payer_addr != &payer_addr {
            return OrderValidationError::payer_addr(order, payer_addr);
        }
        if &order.payee_addr != &payee_addr {
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
    pub batch: Option<BatchMode>,
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
                    batch: msg.batch,
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
        let network_name = network.unwrap_or(driver_details.default_network.to_owned());
        match driver_details.networks.get(&network_name) {
            None => Err(RegisterAccountError::UnsupportedNetwork(
                network_name,
                driver.into(),
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
        let token = token.unwrap_or(network_details.default_token.to_owned());
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

#[derive(Clone)]
pub struct PaymentProcessor {
    db_executor: DbExecutor,
    registry: DriverRegistry,
    in_shutdown: bool,
}

impl PaymentProcessor {
    pub fn new(db_executor: DbExecutor) -> Self {
        Self {
            db_executor,
            registry: Default::default(),
            in_shutdown: false,
        }
    }

    pub async fn register_driver(
        &mut self,
        msg: RegisterDriver,
    ) -> Result<(), RegisterDriverError> {
        self.registry.register_driver(msg)
    }

    pub async fn unregister_driver(&mut self, msg: UnregisterDriver) {
        self.registry.unregister_driver(msg)
    }

    pub async fn register_account(
        &mut self,
        msg: RegisterAccount,
    ) -> Result<(), RegisterAccountError> {
        self.registry.register_account(msg)
    }

    pub async fn unregister_account(&mut self, msg: UnregisterAccount) {
        self.registry.unregister_account(msg)
    }

    pub async fn get_accounts(&self) -> Vec<Account> {
        self.registry.get_accounts()
    }

    pub async fn get_drivers(&self) -> HashMap<String, DriverDetails> {
        self.registry.get_drivers()
    }

    pub async fn get_network(
        &self,
        driver: String,
        network: Option<String>,
    ) -> Result<(String, Network), RegisterAccountError> {
        self.registry.get_network(driver, network)
    }

    pub async fn get_platform(
        &self,
        driver: String,
        network: Option<String>,
        token: Option<String>,
    ) -> Result<String, RegisterAccountError> {
        self.registry.get_platform(driver, network, token)
    }

    pub async fn notify_payment(&self, msg: NotifyPayment) -> Result<(), NotifyPaymentError> {
        let driver = msg.driver;
        let payment_platform = msg.platform;
        let payer_addr = msg.sender;
        let payee_addr = msg.recipient;

        if msg.order_ids.is_empty() {
            return Err(OrderValidationError::new("order_ids is empty").into());
        }
        let (orders, batch_orders) = self
            .db_executor
            .as_dao::<OrderDao>()
            .get_orders(msg.order_ids, driver.clone())
            .await?;
        if !batch_orders.is_empty() {
            for batch_order_item in batch_orders {
                self.db_executor
                    .as_dao::<BatchDao>()
                    .batch_order_item_paid(
                        batch_order_item.id.clone(),
                        batch_order_item.payee_addr.clone(),
                        msg.confirmation.confirmation.clone(),
                    )
                    .await?;
                {
                    let db_order = self
                        .db_executor
                        .as_dao::<BatchDao>()
                        .get_batch_order(batch_order_item.id.clone())
                        .await?;

                    let confirmation = base64::encode(&msg.confirmation.confirmation);

                    let payment = self
                        .db_executor
                        .as_dao::<BatchDao>()
                        .get_batch_order_payments(
                            batch_order_item.id.clone(),
                            batch_order_item.payee_addr.clone(),
                        )
                        .await?;

                    for (payee_id, obligations) in payment.peer_obligation {
                        let payer_id = db_order.owner_id;
                        let payer_addr = payer_addr.clone();
                        let payee_addr = payee_addr.clone();
                        let driver = driver.clone();
                        let payment_platform = payment_platform.clone();
                        let confirmation = confirmation.clone();
                        let agreement_payments = obligations
                            .iter()
                            .filter_map(|obligation: &BatchPaymentObligation| match obligation {
                                BatchPaymentObligation::Invoice {
                                    id,
                                    amount,
                                    agreement_id,
                                } => Some(AgreementPayment {
                                    agreement_id: agreement_id.to_string(),
                                    amount: amount.clone(),
                                    allocation_id: None,
                                }),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        let activity_payments = obligations
                            .iter()
                            .filter_map(|obligation: &BatchPaymentObligation| match obligation {
                                BatchPaymentObligation::DebitNote {
                                    amount,
                                    agreement_id,
                                    activity_id,
                                } => Some(ActivityPayment {
                                    activity_id: activity_id.to_string(),
                                    amount: amount.clone(),
                                    allocation_id: None,
                                }),
                                _ => None,
                            })
                            .collect::<Vec<_>>();

                        Arbiter::spawn(async move {
                            let payment = Payment {
                                payment_id: uuid::Uuid::new_v4().to_string(), // TODO: check this
                                payer_id,
                                payee_id,
                                payer_addr: payer_addr.clone(),
                                payee_addr: payee_addr.clone(),
                                payment_platform,
                                amount: Default::default(),
                                timestamp: Utc::now(),
                                agreement_payments,
                                activity_payments,
                                details: confirmation.clone(),
                            };

                            let signature = match driver_endpoint(&driver)
                                .send(driver::SignPayment(payment.clone()))
                                .await
                            {
                                Ok(Ok(v)) => v,
                                Err(e) => {
                                    log::error!(
                                        "Failed to sign payment: {:?} ,err={:?}",
                                        payment,
                                        e
                                    );
                                    return;
                                }
                                Ok(Err(e)) => {
                                    log::error!("Failed to sign payment: {:?} ,err={}", payment, e);
                                    return;
                                }
                            };

                            let msg = SendPayment::new(payment, signature);

                            match ya_net::from(payer_id)
                                .to(payee_id)
                                .service(BUS_ID)
                                .call(msg)
                                .await
                            {
                                Ok(Ok(v)) => (),
                                Err(e) => log::error!(
                                    "failed to call payment to {}, err={:?}",
                                    payee_id,
                                    e
                                ),
                                Ok(Err(e)) => log::error!(
                                    "failed to send payment to {}, err={:?}",
                                    payee_id,
                                    e
                                ),
                            }
                        })
                    }
                    counter!("payment.amount.sent", ya_metrics::utils::cryptocurrency_to_u64(&msg.amount), "platform" => payment_platform.clone());
                }
            }
        }
        if !orders.is_empty() {
            validate_orders(
                &orders,
                &payment_platform,
                &payer_addr,
                &payee_addr,
                &msg.amount,
            )
            .await?;

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
                    _ => return NotifyPaymentError::invalid_order(&order),
                }
            }

            // FIXME: This is a hack. Payment orders realized by a single transaction are not guaranteed
            //        to have the same payer and payee IDs. Fixing this requires a major redesign of the
            //        data model. Payments can no longer by assigned to a single payer and payee.
            let payer_id = orders.get(0).unwrap().payer_id;
            let payee_id = orders.get(0).unwrap().payee_id;

            let payment_dao: PaymentDao = self.db_executor.as_dao();
            let payment_id = payment_dao
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

            let mut payment = payment_dao.get(payment_id, payer_id).await?.unwrap();
            // Allocation IDs are requestor's private matter and should not be sent to provider
            for agreement_payment in payment.agreement_payments.iter_mut() {
                agreement_payment.allocation_id = None;
            }
            for activity_payment in payment.activity_payments.iter_mut() {
                activity_payment.allocation_id = None;
            }

            let signature = driver_endpoint(&driver)
                .send(driver::SignPayment(payment.clone()))
                .await??;

            counter!("payment.amount.sent", ya_metrics::utils::cryptocurrency_to_u64(&msg.amount), "platform" => payment_platform);
            let msg = SendPayment::new(payment, signature);

            // Spawning to avoid deadlock in a case that payee is the same node as payer
            Arbiter::spawn(
                ya_net::from(payer_id)
                    .to(payee_id)
                    .service(BUS_ID)
                    .call(msg)
                    .map(|res| match res {
                        Ok(Ok(_)) => (),
                        err => log::error!("Error sending payment message to provider: {:?}", err),
                    }),
            );
            // TODO: Implement re-sending mechanism in case SendPayment fails

            counter!("payment.invoices.requestor.paid", 1);
        }
        Ok(())
    }

    pub async fn schedule_payment(&self, msg: SchedulePayment) -> Result<(), SchedulePaymentError> {
        if self.in_shutdown {
            return Err(SchedulePaymentError::Shutdown);
        }
        let amount = msg.amount.clone();
        let driver =
            self.registry
                .driver(&msg.payment_platform, &msg.payer_addr, AccountMode::SEND)?;
        let order_id = driver_endpoint(&driver)
            .send(driver::SchedulePayment::new(
                amount,
                msg.payer_addr.clone(),
                msg.payee_addr.clone(),
                msg.payment_platform.clone(),
                msg.due_date.clone(),
            ))
            .await??;

        self.db_executor
            .as_dao::<OrderDao>()
            .create(msg, order_id, driver)
            .await?;

        Ok(())
    }

    pub async fn verify_payment(
        &self,
        payment: Payment,
        signature: Vec<u8>,
    ) -> Result<(), VerifyPaymentError> {
        // TODO: Split this into smaller functions
        log::info!("Entered verify_payment {:?}, sign={:?}", payment, signature);
        let platform = payment.payment_platform.clone();
        let driver = self.registry.driver(
            &payment.payment_platform,
            &payment.payee_addr,
            AccountMode::RECV,
        )?;

        if !driver_endpoint(&driver)
            .send(driver::VerifySignature::new(payment.clone(), signature))
            .await??
        {
            log::error!(
                "invalid payment signature from: {}/{}",
                payment.payer_id,
                payment.payment_platform
            );
            return Err(VerifyPaymentError::InvalidSignature);
        }

        log::info!("check for confirmation for: {}", payment.payment_id);
        let confirmation = match base64::decode(&payment.details) {
            Ok(confirmation) => PaymentConfirmation { confirmation },
            Err(e) => return Err(VerifyPaymentError::ConfirmationEncoding),
        };
        log::info!("check for driver veryfication for: {}", payment.payment_id);
        let details: PaymentDetails = driver_endpoint(&driver)
            .send(driver::VerifyPayment::new(confirmation, platform.clone()))
            .await??;

        // Verify if amount declared in message matches actual amount transferred on blockchain
        log::info!("check for amount for: {}", payment.payment_id);
        if &details.amount < &payment.amount {
            return VerifyPaymentError::amount(&details.amount, &payment.amount);
        }

        // Verify if payment shares for agreements and activities sum up to the total amount
        let agreement_sum = payment.agreement_payments.iter().map(|p| &p.amount).sum();
        let activity_sum = payment.activity_payments.iter().map(|p| &p.amount).sum();
        if &details.amount < &(&agreement_sum + &activity_sum) {
            log::error!(
                "invalid shares {}, {}, {}",
                &details.amount,
                &agreement_sum,
                &activity_sum
            );
            return VerifyPaymentError::shares(&details.amount, &agreement_sum, &activity_sum);
        }

        let payee_id = payment.payee_id;
        let payer_id = payment.payer_id;
        let payee_addr = &payment.payee_addr;
        let payer_addr = &payment.payer_addr;

        // Verify recipient address
        if &details.recipient != payee_addr {
            log::error!("invalid recipient {}, {}", payee_addr, &details.recipient);
            return VerifyPaymentError::recipient(payee_addr, &details.recipient);
        }
        if &details.sender != payer_addr {
            log::error!("invalid sender {}, {}", payer_addr, &details.sender);
            return VerifyPaymentError::sender(payer_addr, &details.sender);
        }

        // Verify agreement payments
        log::info!("Verify agreement payments {}", payment.payment_id);
        let agreement_dao: AgreementDao = self.db_executor.as_dao();
        for agreement_payment in payment.agreement_payments.iter() {
            let agreement_id = &agreement_payment.agreement_id;
            let agreement = agreement_dao.get(agreement_id.clone(), payee_id).await?;
            match agreement {
                None => return VerifyPaymentError::agreement_not_found(agreement_id),
                Some(agreement) if &agreement.payee_addr != payee_addr => {
                    return VerifyPaymentError::agreement_payee(&agreement, payee_addr);
                }
                Some(agreement) if &agreement.payer_addr != payer_addr => {
                    return VerifyPaymentError::agreement_payer(&agreement, payer_addr);
                }
                Some(agreement) if &agreement.payment_platform != &payment.payment_platform => {
                    return VerifyPaymentError::agreement_platform(
                        &agreement,
                        &payment.payment_platform,
                    );
                }
                _ => (),
            }
        }

        // Verify activity payments
        log::info!("Verify activity payments {}", payment.payment_id);
        let activity_dao: ActivityDao = self.db_executor.as_dao();
        for activity_payment in payment.activity_payments.iter() {
            let activity_id = &activity_payment.activity_id;
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

        // Insert payment into database (this operation creates and updates all related entities)
        log::info!("Insert payment into database {}", payment.payment_id);
        let payment_dao: PaymentDao = self.db_executor.as_dao();
        payment_dao.insert_received(payment, payee_id).await?;
        Ok(())
    }

    pub async fn get_status(
        &self,
        platform: String,
        address: String,
    ) -> Result<BigDecimal, GetStatusError> {
        let driver = self
            .registry
            .driver(&platform, &address, AccountMode::empty())?;
        let amount = driver_endpoint(&driver)
            .send(driver::GetAccountBalance::new(address, platform))
            .await??;
        Ok(amount)
    }

    pub async fn get_gas_balance(
        &self,
        platform: String,
        address: String,
    ) -> Result<Option<GasDetails>, GetStatusError> {
        let driver = self
            .registry
            .driver(&platform, &address, AccountMode::empty())?;
        let amount = driver_endpoint(&driver)
            .send(driver::GetAccountGasBalance::new(address, platform))
            .await??;

        Ok(amount)
    }

    pub async fn validate_allocation(
        &self,
        platform: String,
        address: String,
        amount: BigDecimal,
    ) -> Result<bool, ValidateAllocationError> {
        if self.in_shutdown {
            return Err(ValidateAllocationError::Shutdown);
        }
        let existing_allocations = self
            .db_executor
            .as_dao::<AllocationDao>()
            .get_for_address(platform.clone(), address.clone())
            .await?;
        let driver = self
            .registry
            .driver(&platform, &address, AccountMode::empty())?;
        let msg = ValidateAllocation {
            address,
            platform,
            amount,
            existing_allocations,
        };
        let result = driver_endpoint(&driver).send(msg).await??;
        Ok(result)
    }

    /// This function releases allocations.
    /// When `bool` is `true` all existing allocations are released immediately.
    /// For `false` each allocation timestamp is respected.
    pub async fn release_allocations(&self, force: bool) {
        let db = Data::new(self.db_executor.clone());
        let existing_allocations = db
            .clone()
            .as_dao::<AllocationDao>()
            .get_filtered(None, None, None, None, None)
            .await;

        log::info!("Checking for allocations to be released...");

        match existing_allocations {
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
                    log::info!("No allocations to be released.")
                }
            }
            Err(e) => {
                log::error!("Allocations release failed. Restart yagna to retry allocations release. Db error occurred: {}.", e);
            }
        }
    }

    pub fn shut_down(&mut self, timeout: Duration) -> impl futures::Future<Output = ()> + 'static {
        self.in_shutdown = true;
        let driver_shutdown_futures = self
            .registry
            .iter_drivers()
            .map(|driver| shut_down_driver(driver, timeout));
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
