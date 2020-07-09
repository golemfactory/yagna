use chrono::{DateTime, Utc};
use futures3::lock::Mutex;
use futures3::prelude::*;
use serde_json;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use bigdecimal::BigDecimal;
use ya_client_model::NodeId;
use ya_core_model::driver::{AccountMode, PaymentConfirmation, PaymentDetails};
use ya_payment_driver::{utils, PaymentDriver, PaymentDriverError};
use ya_payment_driver::processor::PaymentDriverProcessor;
use ya_persistence::executor::DbExecutor;
use ya_service_api_interfaces::Provider;

mod service;

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

    fn account_locked<'a>(
        &self,
        _identity: NodeId,
    ) -> Pin<Box<dyn Future<Output = Result<(), PaymentDriverError>> + 'a>> {
        Box::pin(future::ready(Ok(())))
    }

    fn account_unlocked<'a>(
        &self,
        _identity: NodeId,
    ) -> Pin<Box<dyn Future<Output = Result<(), PaymentDriverError>> + 'a>> {
        Box::pin(future::ready(Ok(())))
    }

    fn get_account_balance<'a>(
        &'a self,
        _address: &str,
    ) -> Pin<Box<dyn Future<Output = Result<BigDecimal, PaymentDriverError>> + 'static>> {
        let amount = "1000000000000000000000000";
        Box::pin(future::ready(Ok(utils::str_to_big_dec(&amount).unwrap())))
    }

    fn schedule_payment<'a>(
        &self,
        amount: BigDecimal,
        sender: &str,
        recipient: &str,
        _due_date: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Result<String, PaymentDriverError>> + 'a>> {
        let payments = self.payments.clone();
        let details = PaymentDetails {
            recipient: recipient.to_string(),
            sender: sender.to_string(),
            amount: amount,
            date: Some(Utc::now()),
        };
        // FIXME: generate payment_order_id?
        let payment_order_id = Arc::strong_count(&payments).to_string();

        Box::pin(async move {
            match payments.lock().await.entry(payment_order_id.clone()) {
                // FIXME: need other properties to check if Payment is already Scheduled
                Entry::Occupied(_) => Err(PaymentDriverError::PaymentAlreadyScheduled(payment_order_id)),
                Entry::Vacant(entry) => {
                    entry.insert(details);
                    Ok(payment_order_id)
                }
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
    ) -> Pin<Box<dyn Future<Output = Result<BigDecimal, PaymentDriverError>> + 'static>> {
        let amount = "1000000000000000000000000";
        Box::pin(future::ready(Ok(utils::str_to_big_dec(&amount).unwrap())))
    }
}

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = context.component();
        let driver = DummyDriver::new();
        let processor = PaymentDriverProcessor::new(driver);
        self::service::bind_service(&db, processor);
        self::service::subscribe_to_identity_events().await;
        Ok(())
    }
}
