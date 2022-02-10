use crate::{DRIVER_NAME, NETWORK_NAME, PLATFORM_NAME, TOKEN_NAME};
use actix::Arbiter;
use bigdecimal::BigDecimal;
use chrono::Utc;
use maplit::hashmap;
use std::str::FromStr;
use uuid::Uuid;
use ya_client_model::payment::{DriverDetails, Network};
use ya_core_model::driver::*;
use ya_core_model::payment::local as payment_srv;
use ya_service_bus::typed::service;
use ya_service_bus::{typed as bus, RpcEndpoint};

pub fn bind_service() {
    log::debug!("Binding payment driver service to service bus");

    bus::ServiceBinder::new(&driver_bus_id(DRIVER_NAME), &(), ())
        .bind(init)
        .bind(get_account_balance)
        .bind(schedule_payment)
        .bind(verify_payment)
        .bind(validate_allocation)
        .bind(fund)
        .bind(sign_payment)
        .bind(verify_signature)
        .bind(shut_down);

    log::debug!("Successfully bound payment driver service to service bus");
}

pub async fn register_in_payment_service() -> anyhow::Result<()> {
    log::debug!("Registering driver in payment service...");
    let details = DriverDetails {
        default_network: NETWORK_NAME.to_string(),
        networks: hashmap! {
            NETWORK_NAME.to_string() => Network {
                default_token: TOKEN_NAME.to_string(),
                tokens: hashmap! {
                    TOKEN_NAME.to_string() => PLATFORM_NAME.to_string()
                }
            }
        },
        recv_init_required: false,
    };
    let message = payment_srv::RegisterDriver {
        driver_name: DRIVER_NAME.to_string(),
        details,
    };
    service(payment_srv::BUS_ID).send(message).await?.unwrap(); // Unwrap on purpose because it's NoError
    log::debug!("Successfully registered driver in payment service.");

    Ok(())
}

async fn init(_db: (), _caller: String, msg: Init) -> Result<Ack, GenericError> {
    log::info!("init: {:?}", msg);

    let msg = payment_srv::RegisterAccount {
        address: msg.address(),
        driver: DRIVER_NAME.to_string(),
        network: NETWORK_NAME.to_string(),
        token: TOKEN_NAME.to_string(),
        mode: msg.mode(),
        batch: msg.batch(),
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

async fn schedule_payment(
    _db: (),
    _caller: String,
    msg: SchedulePayment,
) -> Result<String, GenericError> {
    log::info!("schedule payment: {:?}", msg);

    let details = PaymentDetails {
        recipient: msg.recipient(),
        sender: msg.sender(),
        amount: msg.amount(),
        date: Some(Utc::now()),
    };
    let confirmation = serde_json::to_string(&details)
        .map_err(GenericError::new)?
        .into_bytes();
    let order_id = Uuid::new_v4().to_string();
    let msg = payment_srv::NotifyPayment {
        driver: DRIVER_NAME.to_string(),
        platform: PLATFORM_NAME.to_string(),
        amount: details.amount,
        sender: details.sender,
        recipient: details.recipient,
        order_ids: vec![order_id.clone()],
        confirmation: PaymentConfirmation { confirmation },
    };

    // Spawned because calling payment service while handling a call from payment service
    // would result in a deadlock. We need to wait a bit, so parent scope be able to answer
    Arbiter::spawn(async move {
        std::thread::sleep(actix::clock::Duration::from_millis(100));
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

async fn fund(_db: (), _caller: String, _msg: Fund) -> Result<String, GenericError> {
    Ok("Dummy driver is always funded.".to_owned())
}

async fn sign_payment(_db: (), _caller: String, msg: SignPayment) -> Result<Vec<u8>, GenericError> {
    Ok(ya_payment_driver::utils::payment_hash(&msg.0))
}

async fn verify_signature(
    _db: (),
    _caller: String,
    msg: VerifySignature,
) -> Result<bool, GenericError> {
    let hash = ya_payment_driver::utils::payment_hash(&msg.payment);
    Ok(hash == msg.signature)
}

async fn shut_down(_db: (), _caller: String, msg: ShutDown) -> Result<(), GenericError> {
    if msg.timeout > std::time::Duration::from_secs(1) {
        tokio::time::delay_for(msg.timeout - std::time::Duration::from_secs(1)).await;
    }
    Ok(())
}
