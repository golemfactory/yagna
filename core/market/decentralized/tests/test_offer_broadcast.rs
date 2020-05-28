mod utils;

#[cfg(test)]
mod tests {
    use crate::utils::MarketsNetwork;

    use ya_client::model::market::Offer;
    use ya_market_decentralized::MarketService;

    use serde_json::json;
    use std::sync::Arc;

    /// Test adds offer. It should be broadcasted to other nodes in the network.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_broadcast_offer() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("test_broadcast_offer")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?
            .add_market_instance("Node-3")
            .await?;

        // Add Offer on Node-1. It should be propagated to remaining nodes.
        let market1: Arc<MarketService> = network.get_market("Node-1");
        let identity1 = network.get_default_id("Node-1");

        let offer = Offer::new(json!({}), "()".to_string());
        let subscription_id = market1.subscribe_offer(offer, identity1.clone()).await?;

        // Expect, that Offer will appear on other nodes.
        let market2: Arc<MarketService> = network.get_market("Node-2");
        let market3: Arc<MarketService> = network.get_market("Node-3");

        assert!(market2.matcher.get_offer(&subscription_id).await?.is_some());
        assert!(market3.matcher.get_offer(&subscription_id).await?.is_some());

        Ok(())
    }
}
