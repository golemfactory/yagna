use crate::dao::{ActivityDao, AgreementDao, OrderDao, PaymentDao};
use crate::error::processor::{
    AccountNotRegistered, DriverNotRegistered, GetStatusError, NotifyPaymentError,
    OrderValidationError, SchedulePaymentError, VerifyPaymentError,
};
use crate::models::order::ReadObj as DbOrder;
use bigdecimal::{BigDecimal, Zero};
use futures::lock::Mutex;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use ya_client_model::payment::{ActivityPayment, AgreementPayment, Payment};
use ya_core_model::driver::{
    self, driver_bus_id, AccountMode, PaymentConfirmation, PaymentDetails,
};
use ya_core_model::payment::local::{
    NotifyPayment, RegisterAccount, RegisterAccountError, SchedulePayment, UnregisterAccount,
    UnregisterAccountError,
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

    if &total_amount != amount {
        return OrderValidationError::amount(&total_amount, amount);
    }

    Ok(())
}

#[derive(Clone, Default)]
struct DriverRegistry {
    accounts: HashMap<(String, String), (String, AccountMode)>, // (platform, address) -> (driver, mode)
    drivers: HashMap<String, String>,                           // driver -> platform
}

impl DriverRegistry {
    pub fn register_account(&mut self, msg: RegisterAccount) -> Result<(), RegisterAccountError> {
        match self.drivers.entry(msg.driver.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(msg.platform.clone());
            }
            Entry::Occupied(entry) if entry.get() != &msg.platform => {
                return Err(RegisterAccountError::Other(format!(
                    "Driver {} is registered as handling platform {}",
                    msg.driver,
                    entry.get()
                )))
            }
            _ => {}
        }

        match self.accounts.entry((msg.platform, msg.address)) {
            Entry::Occupied(_) => return Err(RegisterAccountError::AlreadyRegistered),
            Entry::Vacant(entry) => entry.insert((msg.driver, msg.mode)),
        };
        Ok(())
    }

    pub fn unregister_account(
        &mut self,
        msg: UnregisterAccount,
    ) -> Result<(), UnregisterAccountError> {
        match self.accounts.remove(&(msg.platform, msg.address)) {
            Some(_) => Ok(()),
            None => Err(UnregisterAccountError::NotRegistered),
        }
    }

    pub fn driver(
        &self,
        platform: &str,
        address: &str,
        mode: AccountMode,
    ) -> Result<String, AccountNotRegistered> {
        match self
            .accounts
            .get(&(platform.to_owned(), address.to_owned()))
        {
            Some((driver, reg_mode)) if reg_mode.contains(mode) => Ok(driver.to_owned()),
            _ => Err(AccountNotRegistered::new(platform, address, mode)),
        }
    }

    pub fn platform(&self, driver: &str) -> Result<String, DriverNotRegistered> {
        match self.drivers.get(driver) {
            Some(platform) => Ok(platform.to_owned()),
            None => Err(DriverNotRegistered::new(driver)),
        }
    }
}

#[derive(Clone)]
pub struct PaymentProcessor {
    db_executor: DbExecutor,
    registry: Arc<Mutex<DriverRegistry>>,
}

impl PaymentProcessor {
    pub fn new(db_executor: DbExecutor) -> Self {
        Self {
            db_executor,
            registry: Default::default(),
        }
    }

    pub async fn register_account(&self, msg: RegisterAccount) -> Result<(), RegisterAccountError> {
        self.registry.lock().await.register_account(msg)
    }

    pub async fn unregister_account(
        &self,
        msg: UnregisterAccount,
    ) -> Result<(), UnregisterAccountError> {
        self.registry.lock().await.unregister_account(msg)
    }

    pub async fn notify_payment(&self, msg: NotifyPayment) -> Result<(), NotifyPaymentError> {
        let payment_platform = self.registry.lock().await.platform(&msg.driver)?;
        let payer_addr = msg.sender;
        let payee_addr = msg.recipient;

        let orders = self
            .db_executor
            .as_dao::<OrderDao>()
            .get_many(msg.order_ids, msg.driver)
            .await?;
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
                payment_platform,
                msg.amount,
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

        let msg = SendPayment(payment);
        ya_net::from(payer_id)
            .to(payee_id)
            .service(BUS_ID)
            .call(msg)
            .await??;

        // TODO: Implement re-sending mechanism in case SendPayment fails

        Ok(())
    }

    pub async fn schedule_payment(&self, msg: SchedulePayment) -> Result<(), SchedulePaymentError> {
        let amount = msg.amount.clone();
        let driver = self.registry.lock().await.driver(
            &msg.payment_platform,
            &msg.payer_addr,
            AccountMode::SEND,
        )?;
        let order_id = driver_endpoint(&driver)
            .send(driver::SchedulePayment::new(
                amount,
                msg.payer_addr.clone(),
                msg.payee_addr.clone(),
                msg.due_date.clone(),
            ))
            .await??;

        self.db_executor
            .as_dao::<OrderDao>()
            .create(msg, order_id, driver)
            .await?;

        Ok(())
    }

    pub async fn verify_payment(&self, payment: Payment) -> Result<(), VerifyPaymentError> {
        // TODO: Split this into smaller functions

        let confirmation = match base64::decode(&payment.details) {
            Ok(confirmation) => PaymentConfirmation { confirmation },
            Err(e) => return Err(VerifyPaymentError::ConfirmationEncoding),
        };
        let driver = self.registry.lock().await.driver(
            &payment.payment_platform,
            &payment.payee_addr,
            AccountMode::RECV,
        )?;
        let details: PaymentDetails = driver_endpoint(&driver)
            .send(driver::VerifyPayment::from(confirmation))
            .await??;

        // Verify if amount declared in message matches actual amount transferred on blockchain
        if &details.amount != &payment.amount {
            return VerifyPaymentError::amount(&details.amount, &payment.amount);
        }

        // Verify if payment shares for agreements and activities sum up to the total amount
        let agreement_sum = payment.agreement_payments.iter().map(|p| &p.amount).sum();
        let activity_sum = payment.activity_payments.iter().map(|p| &p.amount).sum();
        if &details.amount != &(&agreement_sum + &activity_sum) {
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

        // Verify agreement payments
        let agreement_dao: AgreementDao = self.db_executor.as_dao();
        for agreement_payment in payment.agreement_payments.iter() {
            let agreement_id = &agreement_payment.agreement_id;
            let agreement = agreement_dao.get(agreement_id.clone(), payee_id).await?;
            match agreement {
                None => return VerifyPaymentError::agreement_not_found(agreement_id),
                Some(agreement) if &agreement.payee_addr != payee_addr => {
                    return VerifyPaymentError::agreement_payee(&agreement, payee_addr)
                }
                Some(agreement) if &agreement.payer_addr != payer_addr => {
                    return VerifyPaymentError::agreement_payer(&agreement, payer_addr)
                }
                _ => (),
            }
        }

        // Verify activity payments
        let activity_dao: ActivityDao = self.db_executor.as_dao();
        for activity_payment in payment.activity_payments.iter() {
            let activity_id = &activity_payment.activity_id;
            let activity = activity_dao.get(activity_id.clone(), payee_id).await?;
            match activity {
                None => return VerifyPaymentError::activity_not_found(activity_id),
                Some(activity) if &activity.payee_addr != payee_addr => {
                    return VerifyPaymentError::activity_payee(&activity, payee_addr)
                }
                Some(activity) if &activity.payer_addr != payer_addr => {
                    return VerifyPaymentError::activity_payer(&activity, payer_addr)
                }
                _ => (),
            }
        }

        // Verify if transaction hash hasn't been re-used by comparing transaction balance
        // between payer and payee in database and on blockchain
        let db_balance = agreement_dao
            .get_transaction_balance(payee_id, payee_addr.clone(), payer_addr.clone())
            .await?;
        let bc_balance = driver_endpoint(&driver)
            .send(driver::GetTransactionBalance::new(
                payer_addr.clone(),
                payee_addr.clone(),
            ))
            .await??;

        if bc_balance < db_balance + &details.amount {
            return VerifyPaymentError::balance();
        }

        // Insert payment into database (this operation creates and updates all related entities)
        let payment_dao: PaymentDao = self.db_executor.as_dao();
        payment_dao.insert_received(payment, payee_id).await?;

        Ok(())
    }

    pub async fn get_status(
        &self,
        platform: String,
        address: String,
    ) -> Result<BigDecimal, GetStatusError> {
        let driver =
            self.registry
                .lock()
                .await
                .driver(&platform, &address, AccountMode::empty())?;
        let amount = driver_endpoint(&driver)
            .send(driver::GetAccountBalance::from(address))
            .await??;
        Ok(amount)
    }
}
