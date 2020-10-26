// External crates
use actix::Arbiter;
use bigdecimal::BigDecimal;
use chrono::Utc;
use futures3::TryFutureExt;
use std::str::FromStr;
use uuid::Uuid;
use zksync::types::BlockStatus;
use zksync::zksync_types::Address;

// Workspace uses
use ya_core_model::driver::*;
use ya_core_model::payment::local as payment_srv;
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::zksync::unlock_wallet;
use crate::{
    faucet,
    utils::{big_dec_to_big_uint, pack_up},
    zksync::{account_balance, get_wallet},
    DRIVER_NAME, PLATFORM_NAME, ZKSYNC_TOKEN_NAME,
};

pub fn bind_service() {
    log::debug!("Binding payment driver service to service bus");

    bus::ServiceBinder::new(&driver_bus_id(DRIVER_NAME), &(), ())
        .bind(init)
        .bind(get_account_balance)
        .bind(get_transaction_balance)
        .bind(schedule_payment)
        .bind(verify_payment);

    log::debug!("Successfully bound payment driver service to service bus");
}

pub async fn subscribe_to_identity_events() {
    if let Err(e) = bus::service(ya_core_model::identity::BUS_ID)
        .send(ya_core_model::identity::Subscribe {
            endpoint: driver_bus_id(DRIVER_NAME),
        })
        .await
    {
        log::error!("init app-key listener error: {}", e)
    }
}

async fn init(_db: (), _caller: String, msg: Init) -> Result<Ack, GenericError> {
    log::info!("init: {:?}", msg);

    // Hacks required to use zksync/core/model
    let account_depth = std::env::var("ACCOUNT_TREE_DEPTH").unwrap_or("32".to_owned());
    std::env::set_var("ACCOUNT_TREE_DEPTH", account_depth);
    let balance_depth = std::env::var("BALANCE_TREE_DEPTH").unwrap_or("11".to_owned());
    std::env::set_var("BALANCE_TREE_DEPTH", balance_depth);

    let address = msg.address();
    let mode = msg.mode();

    if mode.contains(AccountMode::SEND) {
        faucet::request_ngnt(&address).await?;
        get_wallet(&address).and_then(unlock_wallet).await?;
    }

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
    log::debug!("get account balance: {:?}", msg);

    let addr = Address::from_str(&msg.address()[2..]).map_err(GenericError::new)?;
    Ok(account_balance(addr).await?)
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

    let amount = big_dec_to_big_uint(details.amount.clone())?;
    let amount = pack_up(&amount);
    let wallet = get_wallet(&details.sender).await?;

    let balance = wallet
        .get_balance(BlockStatus::Committed, "GNT")
        .await
        .map_err(GenericError::new)?;
    info!("balance={}", balance);

    let transfer = wallet
        .start_transfer()
        .str_to(&details.recipient[2..])
        .map_err(GenericError::new)?
        .token(ZKSYNC_TOKEN_NAME)
        .map_err(GenericError::new)?
        .amount(amount)
        .send()
        .await
        .map_err(GenericError::new)?;

    log::info!(
        "Created zksync transaction with hash={}",
        hex::encode(transfer.hash())
    );

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
