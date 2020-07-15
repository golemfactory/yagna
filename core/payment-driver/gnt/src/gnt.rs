pub mod common;
pub mod config;
pub mod ethereum;
pub mod faucet;
pub mod sender;

use crate::{GNTDriverError, GNTDriverResult, DRIVER_NAME, PLATFORM_NAME};
use bigdecimal::BigDecimal;
use std::future::Future;
use std::pin::Pin;
use ya_core_model::driver::{AccountMode, PaymentConfirmation};
use ya_core_model::payment::local as payment;
use ya_service_bus::{typed as bus, RpcEndpoint};

pub type SignTx<'a> = &'a (dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>);

pub(crate) async fn notify_payment(
    amount: BigDecimal,
    sender: String,
    recipient: String,
    order_ids: Vec<String>,
    confirmation: PaymentConfirmation,
) -> GNTDriverResult<()> {
    let msg = payment::NotifyPayment {
        driver: DRIVER_NAME.to_string(),
        amount,
        sender,
        recipient,
        order_ids,
        confirmation,
    };

    log::info!("Notify payment: {:?}", msg);
    bus::service(payment::BUS_ID)
        .send(msg)
        .await
        .map_err(|e| GNTDriverError::GSBError(e.to_string()))?
        .map_err(|e| GNTDriverError::LibraryError(e.to_string()))
}

pub(crate) async fn register_account(address: String, mode: AccountMode) -> GNTDriverResult<()> {
    log::info!("Register account: {}, mode: {:?}", address, mode);
    let msg = payment::RegisterAccount {
        platform: PLATFORM_NAME.to_string(),
        address,
        driver: DRIVER_NAME.to_string(),
        mode,
    };
    bus::service(payment::BUS_ID)
        .send(msg)
        .await
        .map_err(|e| GNTDriverError::GSBError(e.to_string()))?
        .map_err(|e| GNTDriverError::LibraryError(e.to_string()))
}
