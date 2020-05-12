mod utils;

/// Use features to disable running market tests in normal cargo test execution.
/// Based on: https://stackoverflow.com/questions/48583049/run-additional-tests-by-using-a-feature-flag-to-cargo-test
///
/// To test market-test-suite run:
/// ```
/// cargo test --manifest-path core/market-decentralized/Cargo.toml --features market-test-suite
/// ```
/// TODO: I don't want to set --manifest-path. How to do this?
#[cfg(test)]
mod tests {
    use tokio;

    use crate::utils::MarketsNetwork;

    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[tokio::test(threaded_scheduler)]
    async fn instantiate() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new()
            .add_market_instance("Node-1")?;

        Ok(())
    }
}
