use crate::dao::{AgreementDao, InvoiceDao, PaymentDao};
use crate::error::{Error, PaymentError, PaymentResult};
use crate::utils::get_sign_tx;
use bigdecimal::BigDecimal;
use std::sync::Arc;
use std::time::Duration;
use ya_client_model::payment::{Invoice, Payment};
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

    async fn process_payment(&self, invoice: Invoice, allocation_id: String) {
        let invoice_id = invoice.invoice_id.clone();
        let payer_id = invoice.recipient_id;
        let payee_id = invoice.issuer_id;

        let result: Result<(), Error> = async move {
            // ************************************** BEGIN **************************************
            // This code is placed here as a temporary workaround because schedule_payment
            // implementation in GNTDriver is waiting for blockchain confirmation.
            // FIXME: Move code below back to PaymentProcessor.schedule_payment
            let invoice_id = invoice.invoice_id.clone();
            let amount = PaymentAmount {
                base_currency_amount: invoice.amount.clone(),
                gas_amount: None,
            };
            // TODO: Allow signing transactions with different key than node ID
            let sign_tx = get_sign_tx(payer_id);
            self.driver
                .schedule_payment(
                    &invoice_id,
                    amount,
                    &invoice.payer_addr,
                    &invoice.payee_addr,
                    invoice.payment_due_date,
                    &sign_tx,
                )
                .await?;
            // *************************************** END ***************************************

            let confirmation = self.wait_for_payment(&invoice.invoice_id).await?;

            let payment_dao: PaymentDao = self.db_executor.as_dao();
            let payment_id = payment_dao
                .create_new(
                    payer_id,
                    invoice.agreement_id,
                    allocation_id,
                    invoice.amount,
                    confirmation.confirmation,
                    vec![],
                    vec![invoice_id.clone()],
                )
                .await?;
            let payment = payment_dao.get(payment_id, payer_id).await?.unwrap();

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
            let invoice_dao: InvoiceDao = self.db_executor.as_dao();
            invoice_dao
                .mark_failed(invoice_id, payer_id)
                .await
                .unwrap_or_else(|e| log::error!("{}", e));
        }
    }

    pub async fn schedule_payment(
        &self,
        invoice: Invoice,
        allocation_id: String,
    ) -> PaymentResult<()> {
        let processor = self.clone();
        tokio::task::spawn_local(async move {
            processor.process_payment(invoice, allocation_id).await;
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

        let actual_amount = details.amount;
        let declared_amount: BigDecimal = payment.amount.clone();
        if actual_amount != declared_amount {
            let msg = format!(
                "Invalid payment amount. Declared: {} Actual: {}",
                declared_amount, actual_amount
            );
            return Err(PaymentError::Verification(msg).into());
        }

        let agreement_id = payment.agreement_id.clone();
        let invoice_ids = payment.invoice_ids.clone().unwrap_or_default();
        let debit_note_ids = payment.debit_note_ids.clone().unwrap_or_default();
        let payee_id = payment.payee_id;

        let invoice_dao: InvoiceDao = self.db_executor.as_dao();
        let invoices = invoice_dao.get_many(invoice_ids, payee_id).await?;
        let total_amount: BigDecimal =
            Iterator::sum(invoices.iter().map(|invoice| invoice.amount.clone()));
        if total_amount != actual_amount {
            let msg = format!(
                "Invalid payment amount. Expected: {} Actual: {}",
                total_amount, actual_amount
            );
            return Err(PaymentError::Verification(msg).into());
        }

        for invoice in invoices.iter() {
            if &invoice.agreement_id != &agreement_id {
                let msg = format!(
                    "Invoice {} has invalid agreement ID. Expected: {} Actual: {}",
                    &invoice.invoice_id, &agreement_id, &invoice.agreement_id
                );
                return Err(PaymentError::Verification(msg).into());
            }
        }

        // TODO: Check debit notes as well

        let payee_addr = payment.payee_addr.clone();
        let payer_addr = payment.payer_addr.clone();
        if &details.recipient != &payee_addr {
            let msg = format!(
                "Invalid transaction recipient. Declared: {} Actual: {}",
                &payee_addr, &details.recipient
            );
            return Err(PaymentError::Verification(msg).into());
        }
        // TODO: Sender should be included in transaction details and checked as well

        let agreement_dao: AgreementDao = self.db_executor.as_dao();
        let agreement = agreement_dao.get(agreement_id.clone(), payee_id).await?;
        match agreement {
            None => {
                let msg = format!("Agreement not found: {}", agreement_id);
                return Err(PaymentError::Verification(msg).into());
            }
            Some(agreement) if &agreement.payee_addr != &payee_addr => {
                let msg = format!(
                    "Invalid payee address. {} != {}",
                    &agreement.payee_addr, &payee_addr
                );
                return Err(PaymentError::Verification(msg).into());
            }
            Some(agreement) if &agreement.payer_addr != &payer_addr => {
                let msg = format!(
                    "Invalid payer address. {} != {}",
                    &agreement.payer_addr, &payer_addr
                );
                return Err(PaymentError::Verification(msg).into());
            }
            _ => (),
        }

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
