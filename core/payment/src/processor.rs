use crate::dao::{ActivityDao, AgreementDao, OrderDao, PaymentDao};
use crate::error::{Error, PaymentError, PaymentResult};
use bigdecimal::{BigDecimal, Zero};
use futures::lock::Mutex;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use ya_client_model::payment::{ActivityPayment, AgreementPayment, Payment};
use ya_core_model::driver::{self, AccountMode, PaymentConfirmation, PaymentDetails};
use ya_core_model::payment::local::{
    GenericError, NotifyPayment, RegisterAccount, RegisterAccountError, SchedulePayment,
    UnregisterAccount, UnregisterAccountError,
};
use ya_core_model::payment::public::{SendPayment, BUS_ID};
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::Endpoint;
use ya_service_bus::{typed as bus, RpcEndpoint};

fn driver_endpoint(driver: &str) -> Endpoint {
    bus::service(driver::BUS_ID_PREFIX.to_string() + driver)
}

#[derive(Clone)]
pub struct PaymentProcessor {
    db_executor: DbExecutor,
    accounts: Arc<Mutex<HashMap<(String, String), (String, AccountMode)>>>, // (platform, address) -> (driver, mode)
    drivers: Arc<Mutex<HashMap<String, String>>>,                           // driver -> platform
}

impl PaymentProcessor {
    pub fn new(db_executor: DbExecutor) -> Self {
        Self {
            db_executor,
            accounts: Default::default(),
            drivers: Default::default(),
        }
    }

    pub async fn register_account(&self, msg: RegisterAccount) -> Result<(), RegisterAccountError> {
        match self.drivers.lock().await.entry(msg.driver.clone()) {
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

        match self
            .accounts
            .lock()
            .await
            .entry((msg.platform, msg.address))
        {
            Entry::Occupied(_) => return Err(RegisterAccountError::AlreadyRegistered),
            Entry::Vacant(entry) => entry.insert((msg.driver, msg.mode)),
        };
        Ok(())
    }

    pub async fn unregister_account(
        &self,
        msg: UnregisterAccount,
    ) -> Result<(), UnregisterAccountError> {
        match self
            .accounts
            .lock()
            .await
            .remove(&(msg.platform, msg.address))
        {
            Some(_) => Ok(()),
            None => Err(UnregisterAccountError::NotRegistered),
        }
    }

    async fn driver(&self, platform: &str, address: &str, mode: AccountMode) -> Option<String> {
        match self
            .accounts
            .lock()
            .await
            .get(&(platform.to_owned(), address.to_owned()))
        {
            Some((driver, reg_mode)) if reg_mode.contains(mode) => Some(driver.to_owned()),
            _ => None,
        }
    }

    pub async fn notify_payment(&self, msg: NotifyPayment) -> Result<(), GenericError> {
        let payment_platform = self
            .drivers
            .lock()
            .await
            .get(&msg.driver)
            .unwrap()
            .to_owned(); // FIXME: Error handling
        let payer_addr = msg.sender;
        let payee_addr = msg.recipient;

        let mut activity_payments = vec![];
        let mut agreement_payments = vec![];
        let mut total_amount = BigDecimal::zero();
        let orders = self
            .db_executor
            .as_dao::<OrderDao>()
            .get_many(msg.order_ids, msg.driver)
            .await
            .unwrap(); // FIXME: Error handling
        for order in orders.iter() {
            if &order.payment_platform != &payment_platform {
                return Err(GenericError::new(format!(
                    "Invalid platform for payment order {}: {} != {}",
                    order.id, order.payment_platform, payment_platform
                )));
            }
            if &order.payer_addr != &payer_addr {
                return Err(GenericError::new(format!(
                    "Invalid sender for payment order {}: {} != {}",
                    order.id, order.payer_addr, payer_addr
                )));
            }
            if &order.payee_addr != &payee_addr {
                return Err(GenericError::new(format!(
                    "Invalid recipient for payment order {}: {} != {}",
                    order.id, order.payee_addr, payee_addr
                )));
            }

            let amount = order.amount.clone().into();
            total_amount += &amount;
            match (order.activity_id.clone(), order.agreement_id.clone()) {
                (Some(activity_id), None) => activity_payments.push(ActivityPayment {
                    activity_id,
                    amount,
                }),
                (None, Some(agreement_id)) => agreement_payments.push(AgreementPayment {
                    agreement_id,
                    amount,
                }),
                _ => {
                    return Err(GenericError::new(format!(
                        "Invalid payment order retrieved from database: {:?}",
                        order
                    )))
                }
            }
        }

        if &total_amount != &msg.amount {
            return Err(GenericError::new(format!(
                "Invalid payment amount: {} != {}",
                total_amount, msg.amount
            )));
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
                "allocation_id".to_owned(), // FIXME
                msg.amount,
                msg.confirmation.confirmation,
                activity_payments,
                agreement_payments,
            )
            .await
            .unwrap(); // FIXME: Error handling

        let payment = payment_dao
            .get(payment_id, payer_id)
            .await
            .unwrap()
            .unwrap(); // FIXME: Error handling
        let msg = SendPayment(payment);
        ya_net::from(payer_id)
            .to(payee_id)
            .service(BUS_ID)
            .call(msg)
            .await
            .unwrap()
            .unwrap(); // FIXME: Error handling
                       // TODO: Implement re-sending mechanism

        Ok(())
    }

    pub async fn schedule_payment(&self, msg: SchedulePayment) -> PaymentResult<()> {
        let amount = msg.amount.clone();
        let driver = self
            .driver(&msg.payment_platform, &msg.payer_addr, AccountMode::SEND)
            .await
            .unwrap(); // FIXME: Error handling
        let order_id = driver_endpoint(&driver)
            .send(driver::SchedulePayment::new(
                amount,
                msg.payer_addr.clone(),
                msg.payee_addr.clone(),
                msg.due_date.clone(),
            ))
            .await
            .unwrap()
            .unwrap(); // FIXME: Error handling

        self.db_executor
            .as_dao::<OrderDao>()
            .create(msg, order_id, driver)
            .await
            .unwrap(); // FIXME: Error handling

        Ok(())
    }

    pub async fn verify_payment(&self, payment: Payment) -> Result<(), Error> {
        let confirmation = match base64::decode(&payment.details) {
            Ok(confirmation) => PaymentConfirmation { confirmation },
            Err(e) => {
                let msg = "Confirmation is not base64-encoded".to_string();
                return Err(PaymentError::Verification(msg).into());
            }
        };
        let driver = self
            .driver(
                &payment.payment_platform,
                &payment.payee_addr,
                AccountMode::RECV,
            )
            .await
            .unwrap(); // FIXME: Error handling
        let details: PaymentDetails = driver_endpoint(&driver)
            .send(driver::VerifyPayment::from(confirmation))
            .await
            .unwrap()
            .unwrap(); // FIXME: Error handling

        // Verify if amount declared in message matches actual amount transferred on blockchain
        let actual_amount = details.amount;
        let declared_amount: BigDecimal = payment.amount.clone();
        if actual_amount != declared_amount {
            let msg = format!(
                "Invalid payment amount. Declared: {} Actual: {}",
                declared_amount, actual_amount
            );
            return Err(PaymentError::Verification(msg).into());
        }

        // Verify if payment shares for agreements and activities sum up to the total amount
        let agreement_payments_total: BigDecimal =
            payment.agreement_payments.iter().map(|p| &p.amount).sum();
        let activity_payments_total: BigDecimal =
            payment.activity_payments.iter().map(|p| &p.amount).sum();
        if actual_amount != (&agreement_payments_total + &activity_payments_total) {
            let msg = format!(
                "Payment shares do not sum up. {} != {} + {}",
                actual_amount, agreement_payments_total, activity_payments_total
            );
            return Err(PaymentError::Verification(msg).into());
        }

        let payee_id = payment.payee_id;
        let payer_id = payment.payer_id;
        let payee_addr = payment.payee_addr.clone();
        let payer_addr = payment.payer_addr.clone();

        // Verify recipient address
        if &details.recipient != &payee_addr {
            let msg = format!(
                "Invalid transaction recipient. Declared: {} Actual: {}",
                &payee_addr, &details.recipient
            );
            return Err(PaymentError::Verification(msg).into());
        }
        // TODO: Sender should be included in transaction details and checked as well

        // Verify agreement payments
        let agreement_dao: AgreementDao = self.db_executor.as_dao();
        for agreement_payment in payment.agreement_payments.iter() {
            let agreement_id = agreement_payment.agreement_id.clone();
            let agreement = agreement_dao.get(agreement_id.clone(), payee_id).await?;
            match agreement {
                None => {
                    let msg = format!("Agreement not found: {}", agreement_id);
                    return Err(PaymentError::Verification(msg).into());
                }
                Some(agreement) if &agreement.payee_addr != &payee_addr => {
                    let msg = format!(
                        "Invalid payee address for agreement {}. {} != {}",
                        agreement_id, &agreement.payee_addr, &payee_addr
                    );
                    return Err(PaymentError::Verification(msg).into());
                }
                Some(agreement) if &agreement.payer_addr != &payer_addr => {
                    let msg = format!(
                        "Invalid payer address for agreement {}. {} != {}",
                        agreement_id, &agreement.payer_addr, &payer_addr
                    );
                    return Err(PaymentError::Verification(msg).into());
                }
                _ => (),
            }
        }

        // Verify activity payments
        let activity_dao: ActivityDao = self.db_executor.as_dao();
        for activity_payment in payment.activity_payments.iter() {
            let activity_id = activity_payment.activity_id.clone();
            let activity = activity_dao.get(activity_id.clone(), payee_id).await?;
            match activity {
                None => {
                    let msg = format!("Activity not found: {}", activity_id);
                    return Err(PaymentError::Verification(msg).into());
                }
                Some(activity) if &activity.payee_addr != &payee_addr => {
                    let msg = format!(
                        "Invalid payee address for activity {}. {} != {}",
                        activity_id, &activity.payee_addr, &payee_addr
                    );
                    return Err(PaymentError::Verification(msg).into());
                }
                Some(activity) if &activity.payer_addr != &payer_addr => {
                    let msg = format!(
                        "Invalid payer address for activity {}. {} != {}",
                        activity_id, &activity.payer_addr, &payer_addr
                    );
                    return Err(PaymentError::Verification(msg).into());
                }
                _ => (),
            }
        }

        // Verify if transaction hash hasn't been re-used by comparing transaction balance
        // between payer and payee in database and on blockchain
        let db_balance = agreement_dao
            .get_transaction_balance(payee_id, payee_addr, payer_addr)
            .await?;
        let bc_balance = driver_endpoint(&driver)
            .send(driver::GetTransactionBalance::new(
                details.sender.clone(),
                details.recipient.clone(),
            ))
            .await
            .unwrap()
            .unwrap();

        if bc_balance < db_balance + actual_amount {
            let msg = "Transaction balance too low (probably tx hash re-used)".to_string();
            return Err(PaymentError::Verification(msg).into());
        }

        // Insert payment into database (this operation creates and updates all related entities)
        let payment_dao: PaymentDao = self.db_executor.as_dao();
        payment_dao.insert_received(payment, payee_id).await?;

        Ok(())
    }

    pub async fn get_status(&self, platform: String, address: String) -> PaymentResult<BigDecimal> {
        let driver = self
            .driver(&platform, &address, AccountMode::empty())
            .await
            .unwrap(); // FIXME: Error handling
        let amount = driver_endpoint(&driver)
            .send(driver::GetAccountBalance::from(address))
            .await
            .unwrap()
            .unwrap(); // FIXME: error handling
        Ok(amount)
    }
}
