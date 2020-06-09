mod utils;

/// Use features to disable running market tests in normal cargo test execution.
/// Based on: https://stackoverflow.com/questions/48583049/run-additional-tests-by-using-a-feature-flag-to-cargo-test
///
/// To test market-test-suite run:
/// ```
/// cargo test --workspace --features ya-market-decentralized/market-test-suite
/// ```
/// or for market crate only
/// ```
/// cargo test -p ya-market-decentralized --features ya-market-decentralized/market-test-suite
/// ```
#[cfg(test)]
mod tests {
    use crate::utils::MarketsNetwork;

    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn instantiate() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("instantiate")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?;

        let _market1 = network.get_market("Node-1");
        let _market2 = network.get_market("Node-2");
        Ok(())
    }
}
