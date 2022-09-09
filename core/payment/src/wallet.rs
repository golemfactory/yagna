// External crates
use bigdecimal::BigDecimal;

// Workspace uses
use ya_core_model::driver::{driver_bus_id, Enter, Exit, Fund, Transfer};
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
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Exit::new(sender, to, amount, network, token);
    let tx_id = bus::service(driver_id).call(message).await??;
    Ok(tx_id)
}

//TODO Rafał
#[allow(clippy::too_many_arguments)]
pub async fn transfer(
    sender: String,
    to: String,
    amount: BigDecimal,
    driver: String,
    network: Option<String>,
    token: Option<String>,
    gas_price: Option<BigDecimal>,
    max_gas_price: Option<BigDecimal>,
    gas_limit: Option<u32>,
    gasless: bool,
) -> anyhow::Result<String> {
    let driver_id = driver_bus_id(driver);
    let message = Transfer::new(
        sender,
        to,
        amount,
        network,
        token,
        gas_price,
        max_gas_price,
        gas_limit,
        gasless,
    );
    let tx_id = bus::service(driver_id).call(message).await??;
    Ok(tx_id)
}
