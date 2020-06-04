mod utils;

#[cfg(test)]
mod tests {
    use crate::utils::mock_offer::{example_demand, example_offer};
    use crate::utils::MarketsNetwork;

    use ya_market_decentralized::MarketService;

    use std::sync::Arc;

    /// Test subscribes offers, checks if offer is available
    /// and than unsubscribes. Checking broadcasting behavior is out of scope.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_subscribe_offer() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("test_subscribe_offer")
            .await
            .add_market_instance("Node-1")
            .await?;

        let market1: Arc<MarketService> = network.get_market("Node-1");
        let identity1 = network.get_default_id("Node-1");

        let mut offer = example_offer();
        let subscription_id = market1.subscribe_offer(&offer, identity1.clone()).await?;

        // Fill expected values for further comparison.
        offer.provider_id = Some(identity1.identity.to_string());
        offer.offer_id = Some(subscription_id.clone());

        // Offer should be available in database after subscribe.
        let got_offer = market1.matcher.get_offer(&subscription_id).await?.unwrap();
        assert_eq!(got_offer, offer);

        // Unsubscribe should fail on not existing subscription id.
        assert_eq!(
            market1
                .unsubscribe_offer("".to_string(), identity1.clone())
                .await
                .is_err(),
            true
        );

        market1
            .unsubscribe_offer(subscription_id.to_string(), identity1.clone())
            .await?;

        // Offer should be removed from database after unsubscribed.
        assert!(market1.matcher.get_offer(&subscription_id).await?.is_none());

        Ok(())
    }

    /// Test subscribes demand, checks if demand is available
    /// and than unsubscribes. Checking broadcasting behavior is out of scope.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_subscribe_demand() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("test_subscribe_demand")
            .await
            .add_market_instance("Node-1")
            .await?;

        let market1: Arc<MarketService> = network.get_market("Node-1");
        let identity1 = network.get_default_id("Node-1");

        let mut demand = example_demand();
        let subscription_id = market1.subscribe_demand(&demand, identity1.clone()).await?;

        // Fill expected values for further comparison.
        demand.requestor_id = Some(identity1.identity.to_string());
        demand.demand_id = Some(subscription_id.clone());

        // Offer should be available in database after subscribe.
        let got_demand = market1.matcher.get_demand(&subscription_id).await?.unwrap();
        assert_eq!(got_demand, demand);

        // Unsubscribe should fail on not existing subscription id.
        assert!(market1
            .unsubscribe_demand("".to_string(), identity1.clone())
            .await
            .is_err());

        market1
            .unsubscribe_demand(subscription_id.to_string(), identity1.clone())
            .await?;

        // Offer should be removed from database after unsubscribed.
        assert!(market1
            .matcher
            .get_demand(&subscription_id)
            .await?
            .is_none());

        Ok(())
    }
}
