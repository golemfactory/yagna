/*
    Driver helper for handling messages from payment_api.
*/
// Extrnal crates
// use lazy_static::lazy_static;
// use num_bigint::BigInt;

// Workspace uses
use ya_payment_driver::{
    driver::BigDecimal,
    model::{GenericError, ValidateAllocation, VerifyPayment},
};

// Local uses
use crate::{
    driver::PaymentDetails,
    erc20::{utils, wallet},
    network,
};

pub async fn verify_payment(msg: VerifyPayment) -> Result<PaymentDetails, GenericError> {
    log::debug!("verify_payment: {:?}", msg);
    let (network, _) = network::platform_to_network_token(msg.platform())?;
    let tx_hash = format!("0x{}", hex::encode(msg.confirmation().confirmation));
    log::info!("Verifying transaction: {}", tx_hash);
    wallet::verify_tx(&tx_hash, network).await
}

pub async fn validate_allocation(msg: ValidateAllocation) -> Result<bool, GenericError> {
    log::debug!("validate_allocation: {:?}", msg);
    let address = utils::str_to_addr(&msg.address)?;
    let (network, _) = network::platform_to_network_token(msg.platform)?;
    let account_balance = wallet::account_balance(address, network).await?;
    let total_allocated_amount: BigDecimal = msg
        .existing_allocations
        .into_iter()
        .map(|allocation| allocation.remaining_amount)
        .sum();

    // TODO: calculate fee. Below commented out reference to zkSync implementation
    // let tx_fee_cost = wallet::get_tx_fee(&msg.address, network).await?;
    // let total_txs_cost = tx_fee_cost * &*TRANSACTIONS_PER_ALLOCATION;
    // let allocation_surcharge = (&*MAX_ALLOCATION_SURCHARGE).min(&total_txs_cost);
    //
    log::info!(
        "Allocation validation: \
        allocating: {:.5}, \
        account_balance: {:.5}, \
        total_allocated_amount: {:.5}", //", \
        //     allocation_surcharge: {:.5} \
        //    ",
        msg.amount,
        account_balance,
        total_allocated_amount,
        //allocation_surcharge,
    );
    // Ok(msg.amount <= (account_balance - total_allocated_amount - allocation_surcharge))
    Ok(msg.amount <= account_balance - total_allocated_amount)
}
