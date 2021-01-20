// External crates
use bigdecimal::BigDecimal;

// Workspace uses
use ya_core_model::driver::{driver_bus_id, Enter, Exit, Transfer};
use ya_service_bus::typed as bus;

pub async fn enter(
    amount: BigDecimal,
    driver: String,
    network: Option<String>,
    token: Option<String>,
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Enter::new(amount, network, token);
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
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Exit::new(sender, to, amount, network, token);
    let tx_id = bus::service(driver_id).call(message).await??;
    Ok(tx_id)
}

pub async fn transfer(
    to: String,
    amount: BigDecimal,
    driver: String,
    network: Option<String>,
    token: Option<String>,
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Transfer::new(to, amount, network, token);
    let tx_id = bus::service(driver_id).call(message).await??;
    Ok(tx_id)
}
