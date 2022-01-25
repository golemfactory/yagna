use web3::types::U256;
use ya_payment_driver::db::models::Network;
use ya_payment_driver::model::GenericError;
use crate::erc20::ethereum::{get_env, get_client};

pub async fn get_network_gas_price(network: Network) -> Result<U256, GenericError>{
    let _env = get_env(network);
    let client = get_client(network)?;

    let small_gas_bump = U256::from(1000);
    let mut gas_price_from_network =
        client.eth().gas_price().await.map_err(GenericError::new)?;

    //add small amount of gas to be first in queue
    if gas_price_from_network / 1000 > small_gas_bump {
        gas_price_from_network += small_gas_bump;
    }
    Ok(gas_price_from_network)
}
