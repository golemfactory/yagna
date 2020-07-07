use chrono::Utc;
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
use ya_core_model::driver::{BUS_ID_PREFIX, AccountMode, PaymentConfirmation, PaymentDetails};
use ya_payment_driver::{service, utils, PaymentDriver, PaymentDriverError};
use ya_payment_driver::processor::PaymentDriverProcessor;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{typed as bus, RpcEndpoint};


const BUS_ID = BUS_ID_PREFIX.to_string() + "dummy";


pub fn bind_service(db: &DbExecutor, processor: PaymentDriverProcessor) {
    log::debug!("Binding payment driver service to service bus");

    bus::ServiceBinder::new(BUS_ID, db, processor)
        .bind_with_processor(account_event)
        .bind_with_processor(init)
        .bind_with_processor(get_account_balance)
        .bind_with_processor(get_payment_status)
        .bind_with_processor(get_transaction_balance)
        .bind_with_processor(schedule_payment)
        .bind_with_processor(verify_payment);

    log::debug!("Successfully bound payment driver service to service bus");
}


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
        Box::pin(future::ready(Ok(BigDecimal {
            base_currency: BigDecimal {
                amount: utils::str_to_big_dec(&amount).unwrap(),
            },
            gas: Some(BigDecimal {
                amount: utils::str_to_big_dec(&amount).unwrap(),
            }),
        })))
    }

    fn schedule_payment<'a>(
        &self,
        amount: BigDecimal,
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

    // fn get_payment_status(
    //     &self,
    //     invoice_id: &str,
    // ) -> Pin<Box<dyn Future<Output = Result<PaymentStatus, PaymentDriverError>> + 'static>> {
    //     let payments = self.payments.clone();
    //     let invoice_id = invoice_id.to_owned();
    //
    //     Box::pin(async move {
    //         match payments.lock().await.get(&invoice_id) {
    //             Some(details) => Ok(PaymentStatus::Ok(PaymentConfirmation::from(
    //                 serde_json::to_string(details).unwrap().as_bytes(),
    //             ))),
    //             None => Err(PaymentDriverError::PaymentNotFound(invoice_id)),
    //         }
    //     })
    // }

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
        Box::pin(future::ready(Ok(BigDecimal {
            amount: utils::str_to_big_dec(&amount).unwrap(),
        })))
    }
}

pub struct PaymentDriverService;

impl PaymentDriverService {
    pub async fn gsb<Context: Provider<Self, DbExecutor>>(context: &Context) -> anyhow::Result<()> {
        let db: DbExecutor = context.component();
        let driver = payment_driver_factory(&db).await?;
        let processor = PaymentDriverProcessor::new(driver);
        self::service::bind_service(&db, processor);
        self::service::subscribe_to_identity_events().await;
        Ok(())
    }
}
