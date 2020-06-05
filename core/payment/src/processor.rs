use crate::dao::{ActivityDao, AgreementDao, PaymentDao};
use crate::error::{Error, PaymentError, PaymentResult};
use crate::utils::get_sign_tx;
use bigdecimal::BigDecimal;
use std::sync::Arc;
use std::time::Duration;
use ya_client_model::payment::{ActivityPayment, AgreementPayment, Payment};
use ya_core_model::payment::local::{PaymentTitle, SchedulePayment};
use ya_core_model::payment::public::{SendPayment, BUS_ID};
use ya_net::RemoteEndpoint;
use ya_payment_driver::{
    AccountBalance, AccountMode, PaymentAmount, PaymentConfirmation, PaymentDriver,
    PaymentDriverError, PaymentStatus,
};
use ya_persistence::executor::DbExecutor;

#[derive(Clone)]
pub struct PaymentProcessor {
    driver: Arc<dyn PaymentDriver + Send + Sync + 'static>,
    db_executor: DbExecutor,
}

impl PaymentProcessor {
    pub fn new<D>(driver: D, db_executor: DbExecutor) -> Self
    where
        D: PaymentDriver + Send + Sync + 'static,
    {
        Self {
            driver: Arc::new(driver),
            db_executor,
        }
    }

    async fn wait_for_payment(&self, invoice_id: &str) -> PaymentResult<PaymentConfirmation> {
        loop {
            match self.driver.get_payment_status(&invoice_id).await? {
                PaymentStatus::Ok(confirmation) => return Ok(confirmation),
                PaymentStatus::NotYet => tokio::time::delay_for(Duration::from_secs(5)).await,
                _ => return Err(PaymentError::Driver(PaymentDriverError::InsufficientFunds)),
            }
        }
    }

    async fn process_payment(&self, msg: SchedulePayment) {
        let payer_id = msg.payer_id;
        let payee_id = msg.payee_id;
        let payer_addr = msg.payer_addr.clone();
        let payee_addr = msg.payee_addr.clone();

        let result: Result<(), Error> = async move {
            let confirmation = self.wait_for_payment(&msg.document_id()).await?;

            let (activity_payments, agreement_payments) = match &msg.title {
                PaymentTitle::DebitNote(debit_note_payment) => (
                    vec![ActivityPayment {
                        activity_id: debit_note_payment.activity_id.clone(),
                        amount: msg.amount.clone(),
                    }],
                    vec![],
                ),
                PaymentTitle::Invoice(invoice_payment) => (
                    vec![],
                    vec![AgreementPayment {
                        agreement_id: invoice_payment.agreement_id.clone(),
                        amount: msg.amount.clone(),
                    }],
                ),
            };

            let payment_dao: PaymentDao = self.db_executor.as_dao();
            let payment_id = payment_dao
                .create_new(
                    payer_id,
                    payee_id,
                    payer_addr,
                    payee_addr,
                    msg.allocation_id,
                    msg.amount,
                    confirmation.confirmation,
                    activity_payments,
                    agreement_payments,
                )
                .await?;
            let payment = payment_dao.get(payment_id, payer_id).await?.unwrap();
            let payee_id = payment.payee_id;
            let msg = SendPayment(payment);
            ya_net::from(payer_id)
                .to(payee_id)
                .service(BUS_ID)
                .call(msg)
                .await??;

            Ok(())
        }
        .await;

        if let Err(e) = result {
            log::error!("Payment failed: {}", e);
        }
    }

    pub async fn schedule_payment(&self, msg: SchedulePayment) -> PaymentResult<()> {
        let document_id = msg.document_id();
        let amount = PaymentAmount {
            base_currency_amount: msg.amount.clone(),
            gas_amount: None,
        };
        // TODO: Allow signing transactions with different key than node ID
        let sign_tx = get_sign_tx(msg.payer_id);
        self.driver
            .schedule_payment(
                &document_id,
                amount,
                &msg.payer_addr,
                &msg.payee_addr,
                msg.due_date,
                &sign_tx,
            )
            .await?;

        let processor = self.clone();
        tokio::task::spawn_local(async move {
            processor.process_payment(msg).await;
        });

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
        let details = self.driver.verify_payment(&confirmation).await?;

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
        let bc_balance = self
            .driver
            .get_transaction_balance(details.sender.as_str(), details.recipient.as_str())
            .await?;
        let bc_balance = bc_balance.amount;
        if bc_balance < db_balance + actual_amount {
            let msg = "Transaction balance too low (probably tx hash re-used)".to_string();
            return Err(PaymentError::Verification(msg).into());
        }

        // Insert payment into database (this operation creates and updates all related entities)
        let payment_dao: PaymentDao = self.db_executor.as_dao();
        payment_dao.insert_received(payment, payee_id).await?;

        Ok(())
    }

    pub async fn init(&self, addr: String, requestor: bool, provider: bool) -> PaymentResult<()> {
        let mut mode = AccountMode::NONE;
        if requestor {
            mode |= AccountMode::SEND;
        }
        if provider {
            mode |= AccountMode::RECV;
        }
        let node_id = addr.parse().unwrap();
        let sign_tx = get_sign_tx(node_id);
        Ok(self.driver.init(mode, addr.as_str(), &sign_tx).await?)
    }

    pub async fn get_status(&self, addr: &str) -> PaymentResult<BigDecimal> {
        let balance: AccountBalance = self.driver.get_account_balance(addr).await?;
        Ok(balance.base_currency.amount)
    }
}
