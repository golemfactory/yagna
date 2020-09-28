// External crates
use actix::Arbiter;
use bigdecimal::BigDecimal;
use chrono::Utc;
use client::rpc_client::RpcClient;
use client::wallet::{BalanceState, Wallet};
use num::BigUint;
use std::str::FromStr;
use uuid::Uuid;
use web3::types::Address;

// Workspace uses
use ya_core_model::driver::*;
use ya_core_model::payment::local as payment_srv;
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::zksync::{eth_sign_transfer, get_zksync_seed, PackedEthSignature};
use crate::{DRIVER_NAME, PLATFORM_NAME};

const ZKSYNC_RPC_ADDRESS: &'static str = "https://rinkeby-api.zksync.io/jsrpc";
const ZKSYNC_TOKEN_NAME: &'static str = "GNT";

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
    log::debug!("get account balance: {:?}", msg);

    let pub_address = Address::from_str(&msg.address()[2..]).unwrap();
    let provider = RpcClient::new(ZKSYNC_RPC_ADDRESS);
    let wallet = Wallet::from_public_address(pub_address, provider);
    let balance_com = wallet
        .get_balance(ZKSYNC_TOKEN_NAME, BalanceState::Committed)
        .await;

    log::debug!("balance: {}", balance_com);
    Ok(BigDecimal::from_str(&balance_com.to_string()).unwrap())
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

    let pub_address = Address::from_str(&details.sender[2..]).unwrap();
    let seed = get_zksync_seed(pub_address).await;
    let provider = RpcClient::new(ZKSYNC_RPC_ADDRESS);
    let wallet = Wallet::from_seed(seed, pub_address, provider);

    let recepient = Address::from_str(&details.recipient).unwrap();
    let amount = BigUint::from_str(&details.amount.to_string()).unwrap();
    let (tx, msg) = wallet
        .prepare_sync_transfer(&recepient, ZKSYNC_TOKEN_NAME.to_string(), amount, None)
        .await;
    let signed_msg = eth_sign_transfer(pub_address, msg).await;
    let packed_sig = PackedEthSignature::deserialize_packed(&signed_msg).unwrap();
    wallet.sync_transfer(tx, packed_sig).await;

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
