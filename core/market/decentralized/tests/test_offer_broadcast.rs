mod utils;

#[cfg(test)]
mod tests {
    use crate::utils::mock_node::default::*;
    use crate::utils::mock_offer::example_offer;
    use crate::utils::MarketsNetwork;

    use ya_client::model::market::Offer;
    use ya_market_decentralized::protocol::Discovery;
    use ya_market_decentralized::Offer as ModelOffer;
    use ya_market_decentralized::{MarketService, SubscriptionId};

    use serde_json::json;
    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;

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

        Ok(())
    }

    /// Offer subscription id should be validated on reception. If Offer
    /// id hash doesn't match hash computed from Offer fields, Market should
    /// reject such an Offer since it could be some kind of attack.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_broadcast_offer_validation() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("test_broadcast_offer_validation")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_discovery_instance(
                "Node-2",
                empty_on_offer_received,
                empty_on_offer_unsubscribed,
                empty_on_retrieve_offers,
            )
            .await?;

        let market1: Arc<MarketService> = network.get_market("Node-1");
        let market2: Discovery = network.get_discovery("Node-2");
        let identity2 = network.get_default_id("Node-2");

        // Prepare Offer with subscription id changed to invalid.
        let false_subscription_id = SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53")?;
        let offer = example_offer();
        let mut offer = ModelOffer::from_new(&offer, &identity2);
        offer.id = false_subscription_id.clone();

        // Offer should be propagated to market1, but he should reject it.
        market2.broadcast_offer(offer).await?;
        tokio::time::delay_for(Duration::from_secs(1)).await;

        assert!(market1
            .matcher
            .get_offer(false_subscription_id.to_string())
            .await?
            .is_none());
        Ok(())
    }
}
