use ya_core_model::driver::{driver_bus_id, Enter, Exit, Transfer};
use ya_service_bus::typed as bus;

pub async fn enter(
    amount: String,
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
    to: Option<String>,
    amount: Option<String>,
    driver: String,
    network: Option<String>,
    token: Option<String>,
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Exit::new(to, amount, network, token);
    let tx_id = bus::service(driver_id).call(message).await??;
    Ok(tx_id)
}

pub async fn transfer(
    to: String,
    amount: String,
    driver: String,
    network: Option<String>,
    token: Option<String>,
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Transfer::new(to, amount, network, token);
    let tx_id = bus::service(driver_id).call(message).await??;
    Ok(tx_id)
}
