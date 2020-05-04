use crate::dao::{DebitNoteDao, InvoiceDao, PaymentDao};
use crate::error::{Error, PaymentError, PaymentResult};
use crate::models as db_models;
use crate::utils::get_sign_tx;
use bigdecimal::BigDecimal;
use ethereum_types::{Address, U256};
use futures::lock::Mutex;
use num_bigint::ToBigInt;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;
use ya_client_model::payment::{Invoice, InvoiceStatus, Payment};
use ya_core_model::ethaddr::NodeId;
use ya_core_model::payment::public::{SendPayment, BUS_ID};
use ya_net::TryRemoteEndpoint;
use ya_payment_driver::{
    AccountBalance, AccountMode, PaymentAmount, PaymentConfirmation, PaymentDriver,
    PaymentDriverError, PaymentStatus,
};
use ya_persistence::executor::DbExecutor;

const PRECISION: u64 = 1_000_000_000_000_000_000;
const GAS_LIMIT: u64 = 1_000_000_000_000_000_000; // TODO: Handle gas limits otherwise

#[derive(Clone)]
pub struct PaymentProcessor {
    driver: Arc<Mutex<Box<dyn PaymentDriver + Send + Sync + 'static>>>,
    db_executor: DbExecutor,
}

fn str_to_addr(addr: &str) -> PaymentResult<Address> {
    match addr.trim_start_matches("0x").parse() {
        Ok(addr) => Ok(addr),
        Err(e) => Err(PaymentError::Address(addr.to_string())),
    }
}

fn addr_to_str(addr: Address) -> String {
    format!("0x{}", hex::encode(addr.to_fixed_bytes()))
}

fn big_dec_to_u256(v: BigDecimal) -> PaymentResult<U256> {
    let v = v * Into::<BigDecimal>::into(PRECISION);
    let v = v.to_bigint().unwrap();
    let v = &v.to_string();
    Ok(U256::from_dec_str(v)?)
}

fn u256_to_big_dec(v: U256) -> PaymentResult<BigDecimal> {
    let v: BigDecimal = v.to_string().parse()?;
    Ok(v / Into::<BigDecimal>::into(PRECISION))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_conversion() {
        let amount_str = "10000.123456789012345678";
        let big_dec: BigDecimal = amount_str.parse().unwrap();
        let u256 = big_dec_to_u256(big_dec.clone()).unwrap();
        assert_eq!(big_dec, u256_to_big_dec(u256).unwrap());
    }

    #[test]
    fn test_address_conversion() {
        let addr_str = "0xd39a168f0480b8502c2531b2ffd8588c592d713a";
        let addr = str_to_addr(addr_str).unwrap();
        assert_eq!(addr_str, addr_to_str(addr));
    }
}

impl PaymentProcessor {
    pub fn new<D>(driver: D, db_executor: DbExecutor) -> Self
    where
        D: PaymentDriver + Send + Sync + 'static,
    {
        Self {
            driver: Arc::new(Mutex::new(Box::new(driver))),
            db_executor,
        }
    }

    async fn wait_for_payment(&self, invoice_id: &str) -> PaymentResult<PaymentConfirmation> {
        loop {
            match self
                .driver
                .lock()
                .await
                .get_payment_status(&invoice_id)
                .await?
            {
                PaymentStatus::Ok(confirmation) => return Ok(confirmation),
                PaymentStatus::NotYet => tokio::time::delay_for(Duration::from_secs(5)).await,
                _ => return Err(PaymentError::Driver(PaymentDriverError::InsufficientFunds)),
            }
        }
    }

    async fn process_payment(&self, invoice: Invoice, allocation_id: String) {
        let invoice_id = invoice.invoice_id.clone();
        let result: Result<(), Error> = async move {
            // ************************************** BEGIN **************************************
            // This code is placed here as a temporary workaround because schedule_payment
            // implementation in GNTDriver is waiting for blockchain confirmation.
            // FIXME: Move code below back to PaymentProcessor.schedule_payment
            let invoice_id = invoice.invoice_id.clone();
            let amount = PaymentAmount {
                base_currency_amount: big_dec_to_u256(invoice.amount.clone())?,
                gas_amount: None,
            };
            // TODO: Allow signing transactions with different key than node ID
            let sender = str_to_addr(&invoice.recipient_id)?;
            let recipient = str_to_addr(&invoice.credit_account_id)?;
            let sign_tx = get_sign_tx(invoice.recipient_id.parse().unwrap());
            self.driver
                .lock()
                .await
                .schedule_payment(
                    &invoice_id,
                    amount,
                    sender,
                    recipient,
                    invoice.payment_due_date,
                    &sign_tx,
                )
                .await?;
            // *************************************** END ***************************************

            let confirmation = self.wait_for_payment(&invoice.invoice_id).await?;

            let payment_id = Uuid::new_v4().to_string();
            let payment = db_models::BareNewPayment {
                id: payment_id.clone(),
                payer_id: invoice.recipient_id,
                payee_id: invoice.issuer_id,
                amount: invoice.amount.into(),
                allocation_id: Some(allocation_id),
                details: confirmation.confirmation,
            };
            let payment = db_models::NewPayment {
                payment,
                debit_note_ids: vec![],
                invoice_ids: vec![invoice.invoice_id.clone()],
            };
            let payment_dao: PaymentDao = self.db_executor.as_dao();
            payment_dao.create(payment).await?;
            let payment = payment_dao.get(payment_id).await?.unwrap();

            let payee_id: NodeId = payment.payment.payee_id.parse().unwrap();
            let msg = SendPayment(payment.into());
            payee_id
                .try_service(BUS_ID)
                .unwrap() //FIXME
                .call(msg)
                .await??;

            let invoice_dao: InvoiceDao = self.db_executor.as_dao();
            invoice_dao
                .update_status(invoice.invoice_id, InvoiceStatus::Settled.into())
                .await?;
            Ok(())
        }
        .await;

        if let Err(e) = result {
            log::error!("Payment failed: {}", e);
            let invoice_dao: InvoiceDao = self.db_executor.as_dao();
            invoice_dao
                .update_status(invoice_id, InvoiceStatus::Failed.into())
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
        let payment: db_models::Payment = payment.into();
        let confirmation = PaymentConfirmation {
            confirmation: payment.payment.details.clone(),
        };
        let details = self
            .driver
            .lock()
            .await
            .verify_payment(&confirmation)
            .await?;

        let actual_amount = u256_to_big_dec(details.amount)?;
        let declared_amount: BigDecimal = payment.payment.amount.clone().into();
        if actual_amount != declared_amount {
            let msg = format!(
                "Invalid payment amount. Declared: {} Actual: {}",
                declared_amount, actual_amount
            );
            return Err(PaymentError::Verification(msg).into());
        }

        let invoice_ids = payment.invoice_ids.clone();
        let debit_note_ids = payment.debit_note_ids.clone();

        let invoice_dao: InvoiceDao = self.db_executor.as_dao();
        let total_amount = invoice_dao.get_total_amount(invoice_ids.clone()).await?;
        if total_amount != actual_amount {
            let msg = format!(
                "Invalid payment amount. Expected: {} Actual: {}",
                total_amount, actual_amount
            );
            return Err(PaymentError::Verification(msg).into());
        }

        // Translate account ids to lower case, because recipient will be address without checksum.
        let recipient = addr_to_str(details.recipient);
        let account_ids = invoice_dao
            .get_accounts_ids(invoice_ids.clone())
            .await?
            .iter()
            .map(|account| account.to_lowercase())
            .collect::<Vec<String>>();
        log::debug!("Recipient: {}, account_ids: {:?}", recipient, account_ids);
        if account_ids != [recipient.clone()] {
            return Err(
                PaymentError::Verification(format!("Invalid account ID: {}", recipient)).into(),
            );
        }

        // TODO: Check debit notes as well
        // It's not as simple as with invoices because debit notes contain total amount due.
        // Probably payments should be related to agreements not particular invoices/debit notes.

        // FIXME: This code assumes that payer always uses the same Ethereum address
        let payment_dao: PaymentDao = self.db_executor.as_dao();
        let db_balance = payment_dao
            .get_transaction_balance(payment.payment.payer_id.clone())
            .await?;
        let bc_balance = self
            .driver
            .lock()
            .await
            .get_transaction_balance(details.sender, details.recipient)
            .await?;
        let bc_balance = u256_to_big_dec(bc_balance.amount)?;

        if bc_balance < db_balance + actual_amount {
            let msg = "Transaction balance too low (probably tx hash re-used)".to_string();
            return Err(PaymentError::Verification(msg).into());
        }

        payment_dao.create(payment.into()).await?;

        let debit_note_dao: DebitNoteDao = self.db_executor.as_dao();
        for debit_note_id in debit_note_ids {
            debit_note_dao
                .update_status(debit_note_id, InvoiceStatus::Settled.into())
                .await?;
        }
        for invoice_id in invoice_ids {
            invoice_dao
                .update_status(invoice_id, InvoiceStatus::Settled.into())
                .await?;
        }

        Ok(())
    }

    pub async fn init(&self, addr: Address, requestor: bool, provider: bool) -> PaymentResult<()> {
        let mut mode = AccountMode::NONE;
        if requestor {
            mode |= AccountMode::SEND;
        }
        if provider {
            mode |= AccountMode::RECV;
        }
        let node_id = addr_to_str(addr).parse().unwrap();
        let sign_tx = get_sign_tx(node_id);
        Ok({ self.driver.lock().await.init(mode, addr, &sign_tx) }.await?)
    }

    pub async fn get_status(&self, addr: Address) -> PaymentResult<BigDecimal> {
        let balance: AccountBalance =
            { self.driver.lock().await.get_account_balance(addr) }.await?;

        Ok(u256_to_big_dec(balance.base_currency.amount)?)
    }
}
