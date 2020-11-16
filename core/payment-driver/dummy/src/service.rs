use crate::{DRIVER_NAME, PLATFORM_NAME};
use actix::Arbiter;
use bigdecimal::BigDecimal;
use chrono::Utc;
use std::str::FromStr;
use uuid::Uuid;
use ya_core_model::driver::*;
use ya_core_model::payment::local as payment_srv;
use ya_service_bus::{typed as bus, RpcEndpoint};

pub fn bind_service() {
    log::debug!("Binding payment driver service to service bus");

    bus::ServiceBinder::new(&driver_bus_id(DRIVER_NAME), &(), ())
        .bind(init)
        .bind(get_account_balance)
        .bind(get_transaction_balance)
        .bind(schedule_payment)
        .bind(verify_payment)
        .bind(validate_allocation);

    log::debug!("Successfully bound payment driver service to service bus");
}

async fn init(_db: (), _caller: String, msg: Init) -> Result<Ack, GenericError> {
    log::info!("init: {:?}", msg);

    let address = msg.address();
    let mode = msg.mode();

    let msg = payment_srv::RegisterAccount {
        platform: PLATFORM_NAME.to_string(),
        address,
        driver: DRIVER_NAME.to_string(),
        mode,
    };
    bus::service(payment_srv::BUS_ID)
        .send(msg)
        .await
        .map_err(GenericError::new)?
        .map_err(GenericError::new)?;
    Ok(Ack {})
}

async fn get_account_balance(
    _db: (),
    _caller: String,
    msg: GetAccountBalance,
) -> Result<BigDecimal, GenericError> {
    log::info!("get account balance: {:?}", msg);

    BigDecimal::from_str("1000000000000000000000000").map_err(GenericError::new)
}

async fn get_transaction_balance(
    _db: (),
    _caller: String,
    msg: GetTransactionBalance,
) -> Result<BigDecimal, GenericError> {
    log::info!("get transaction balance: {:?}", msg);

    BigDecimal::from_str("1000000000000000000000000").map_err(GenericError::new)
}

async fn schedule_payment(
    _db: (),
    _caller: String,
    msg: SchedulePayment,
) -> Result<String, GenericError> {
    log::info!("schedule payment: {:?}", msg);

    let details = PaymentDetails {
        recipient: msg.recipient().to_string(),
        sender: msg.sender().to_string(),
        amount: msg.amount(),
        date: Some(Utc::now()),
    };
    let confirmation = serde_json::to_string(&details)
        .map_err(GenericError::new)?
        .into_bytes();
    let order_id = Uuid::new_v4().to_string();
    let msg = payment_srv::NotifyPayment {
        driver: DRIVER_NAME.to_string(),
        amount: details.amount,
        sender: details.sender,
        recipient: details.recipient,
        order_ids: vec![order_id.clone()],
        confirmation: PaymentConfirmation { confirmation },
    };

    // Spawned because calling payment service while handling a call from payment service
    // would result in a deadlock.
    Arbiter::spawn(async move {
        let _ = bus::service(payment_srv::BUS_ID)
            .send(msg)
            .await
            .map_err(|e| log::error!("{}", e));
    });

    Ok(order_id)
}

async fn verify_payment(
    _db: (),
    _caller: String,
    msg: VerifyPayment,
) -> Result<PaymentDetails, GenericError> {
    log::info!("verify payment: {:?}", msg);

    let confirmation = msg.confirmation();
    let json_str = std::str::from_utf8(confirmation.confirmation.as_slice()).unwrap();
    let details = serde_json::from_str(&json_str).unwrap();
    Ok(details)
}

async fn validate_allocation(
    _db: (),
    _caller: String,
    _msg: ValidateAllocation,
) -> Result<bool, GenericError> {
    Ok(true)
}
