pub mod common;
pub mod config;
pub mod ethereum;
pub mod faucet;
pub mod sender;

use crate::{GNTDriverResult, DRIVER_NAME, PLATFORM_NAME};
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
    let _ = bus::service(payment::BUS_ID)
        .send(msg)
        .await
        .unwrap()
        .unwrap();
    Ok(())
}

pub(crate) async fn register_account(address: String, mode: AccountMode) -> GNTDriverResult<()> {
    log::info!("Register account: {}, mode: {:?}", address, mode);
    let msg = payment::RegisterAccount {
        platform: PLATFORM_NAME.to_string(),
        address,
        driver: DRIVER_NAME.to_string(),
        mode,
    };
    let _ = bus::service(payment::BUS_ID)
        .send(msg)
        .await
        .unwrap()
        .unwrap();
    Ok(())
}
