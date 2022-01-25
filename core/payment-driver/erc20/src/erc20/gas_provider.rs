use web3::types::U256;
use ya_payment_driver::db::models::Network;
use ya_payment_driver::model::GenericError;
use crate::erc20::ethereum::{get_env, get_network_gas_price_eth};

pub async fn get_network_gas_price(network: Network) -> Result<U256, GenericError>{
    let env = get_env(network);
    if env.use_external_gas_provider {
        Err(GenericError::new("TODO - implement external gas provider"))
    } else {
        //use internal gas provider (from Geth/Bor node)
        let gas_price_internal = get_network_gas_price_eth(network).await?;

        //test - set starting price as half of current network price
        Ok(gas_price_internal / U256::from(2))
    }
}
