// External crates
use bigdecimal::BigDecimal;

// Workspace uses
use ya_core_model::driver::{driver_bus_id, Enter, Exit, ExitFee, FeeResult, Fund, Transfer};
use ya_service_bus::typed as bus;

pub async fn fund(
    address: String,
    driver: String,
    network: Option<String>,
    token: Option<String>,
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Fund::new(address, network, token);
    let reply = bus::service(driver_id).call(message).await??;
    Ok(reply)
}

pub async fn enter(
    amount: BigDecimal,
    address: String,
    driver: String,
    network: Option<String>,
    token: Option<String>,
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Enter::new(amount, address, network, token);
    let tx_id = bus::service(driver_id).call(message).await??;
    Ok(tx_id)
}

pub async fn exit(
    sender: String,
    to: Option<String>,
    amount: Option<BigDecimal>,
    driver: String,
    network: Option<String>,
    token: Option<String>,
    fee_limit: Option<BigDecimal>,
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Exit::new(sender, to, amount, network, token, fee_limit);
    let tx_id = bus::service(driver_id).call(message).await??;
    Ok(tx_id)
}

pub async fn transfer(
    sender: String,
    to: String,
    amount: BigDecimal,
    driver: String,
    network: Option<String>,
    token: Option<String>,
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Transfer::new(sender, to, amount, network, token);
    let tx_id = bus::service(driver_id).call(message).await??;
    Ok(tx_id)
}

pub async fn exit_fee(
    sender: String,
    to: Option<String>,
    amount: Option<BigDecimal>,
    driver: String,
    network: Option<String>,
    token: Option<String>,
) -> anyhow::Result<FeeResult> {
    let driver_id = driver_bus_id(driver);
    Ok(bus::service(driver_id)
        .call(ExitFee {
            sender,
            to,
            amount,
            network,
            token,
        })
        .await??)
}
