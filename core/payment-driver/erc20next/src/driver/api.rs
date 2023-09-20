/*
    Driver helper for handling messages from payment_api.
*/
// Extrnal crates
// use lazy_static::lazy_static;
// use num_bigint::BigInt;

// Workspace uses
use ya_payment_driver::model::{GenericError, VerifyPayment};

// Local uses
use crate::{driver::PaymentDetails, erc20::wallet, network};

pub async fn verify_payment(msg: VerifyPayment) -> Result<PaymentDetails, GenericError> {
    log::debug!("verify_payment: {:?}", msg);
    let (network, _) = network::platform_to_network_token(msg.platform())?;
    let tx_hash = format!("0x{}", hex::encode(msg.confirmation().confirmation));
    log::info!("Verifying transaction: {}", tx_hash);
    wallet::verify_tx(&tx_hash, network).await
}
