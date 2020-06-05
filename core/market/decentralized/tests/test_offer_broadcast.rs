mod utils;

#[cfg(test)]
mod tests {
    use crate::utils::MarketsNetwork;

    use ya_client::model::market::Offer;
    use ya_market_decentralized::MarketService;

    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;

    /// Test adds offer. It should be broadcasted to other nodes in the network.
    /// Than sending unsubscribe should remove Offer from other nodes.
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

        let mut offer = Offer::new(json!({}), "()".to_string());
        let subscription_id = market1.subscribe_offer(&offer, identity1.clone()).await?;

        // Fill expected values for further comparison.
        offer.provider_id = Some(identity1.identity.to_string());
        offer.offer_id = Some(subscription_id.clone());

        // Expect, that Offer will appear on other nodes.
        let market2: Arc<MarketService> = network.get_market("Node-2");
        let market3: Arc<MarketService> = network.get_market("Node-3");

        // Wait for Offer propagation.
        // TODO: How to wait without assuming any number of seconds?
        tokio::time::delay_for(Duration::from_secs(1)).await;

        assert_eq!(
            offer,
            market2.matcher.get_offer(&subscription_id).await?.unwrap()
        );
        assert_eq!(
            offer,
            market3.matcher.get_offer(&subscription_id).await?.unwrap()
        );

        // Unsubscribe Offer. Wait some delay for propagation.
        market1
            .unsubscribe_offer(subscription_id.clone(), identity1.clone())
            .await?;
        tokio::time::delay_for(Duration::from_secs(1)).await;

        // We expect, that Offers won't be available on other nodes now
        assert!(market2.matcher.get_offer(&subscription_id).await?.is_none());
        assert!(market3.matcher.get_offer(&subscription_id).await?.is_none());

        Ok(())
    }
}
