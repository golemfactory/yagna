use crate::{utils, PaymentDriver, PaymentDriverError};
use chrono::{DateTime, Utc};
use futures3::lock::Mutex;
use futures3::prelude::*;
use serde_json;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use ya_core_model::driver::{
    AccountBalance, AccountMode, Balance, Currency, PaymentAmount, PaymentConfirmation,
    PaymentDetails, PaymentStatus,
};

#[derive(Clone)]
pub struct DummyDriver {
    payments: Arc<Mutex<HashMap<String, PaymentDetails>>>,
}

impl DummyDriver {
    pub fn new() -> Self {
        Self {
            payments: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl PaymentDriver for DummyDriver {
    fn init<'a>(
        &self,
        _mode: AccountMode,
        _address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), PaymentDriverError>> + 'a>> {
        Box::pin(future::ready(Ok(())))
    }

    fn get_account_balance<'a>(
        &'a self,
        _address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<AccountBalance, PaymentDriverError>> + 'static>> {
        let amount = "1000000000000000000000000";
        Box::pin(future::ready(Ok(AccountBalance {
            base_currency: Balance {
                currency: Currency::Gnt,
                amount: utils::str_to_big_dec(&amount).unwrap(),
            },
            gas: Some(Balance {
                currency: Currency::Eth,
                amount: utils::str_to_big_dec(&amount).unwrap(),
            }),
        })))
    }

    fn schedule_payment<'a>(
        &self,
        invoice_id: &str,
        amount: PaymentAmount,
        sender: &str,
        recipient: &str,
        _due_date: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Result<(), PaymentDriverError>> + 'a>> {
        let payments = self.payments.clone();
        let details = PaymentDetails {
            recipient: recipient.to_string(),
            sender: sender.to_string(),
            amount: amount.base_currency_amount,
            date: Some(Utc::now()),
        };
        let invoice_id = invoice_id.to_string();

        Box::pin(async move {
            match payments.lock().await.entry(invoice_id.clone()) {
                Entry::Occupied(_) => Err(PaymentDriverError::PaymentAlreadyScheduled(invoice_id)),
                Entry::Vacant(entry) => {
                    entry.insert(details);
                    Ok(())
                }
            }
        })
    }

    fn reschedule_payment<'a>(
        &self,
        invoice_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), PaymentDriverError>> + 'a>> {
        let invoice_id = invoice_id.to_owned();
        let payments = self.payments.clone();

        Box::pin(async move {
            match payments.lock().await.get(&invoice_id) {
                Some(_) => Ok(()),
                None => Err(PaymentDriverError::PaymentNotFound(invoice_id)),
            }
        })
    }

    fn get_payment_status(
        &self,
        invoice_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<PaymentStatus, PaymentDriverError>> + 'static>> {
        let payments = self.payments.clone();
        let invoice_id = invoice_id.to_owned();

        Box::pin(async move {
            match payments.lock().await.get(&invoice_id) {
                Some(details) => Ok(PaymentStatus::Ok(PaymentConfirmation::from(
                    serde_json::to_string(details).unwrap().as_bytes(),
                ))),
                None => Err(PaymentDriverError::PaymentNotFound(invoice_id)),
            }
        })
    }

    fn verify_payment<'a>(
        &'a self,
        confirmation: &PaymentConfirmation,
    ) -> Pin<Box<dyn Future<Output = Result<PaymentDetails, PaymentDriverError>> + 'static>> {
        let json_str = std::str::from_utf8(confirmation.confirmation.as_slice()).unwrap();
        let details = serde_json::from_str(&json_str).unwrap();
        Box::pin(future::ready(Ok(details)))
    }

    fn get_transaction_balance<'a>(
        &'a self,
        _payer: &str,
        _payee: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Balance, PaymentDriverError>> + 'static>> {
        let amount = "1000000000000000000000000";
        Box::pin(future::ready(Ok(Balance {
            currency: Currency::Gnt,
            amount: utils::str_to_big_dec(&amount).unwrap(),
        })))
    }
}
