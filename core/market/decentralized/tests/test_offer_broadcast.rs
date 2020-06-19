mod utils;

#[cfg(test)]
mod tests {
    use crate::utils::mock_node::{default::*, wait_for_bcast};
    use crate::utils::mock_offer::example_offer;
    use crate::utils::MarketsNetwork;

    use ya_client::model::market::Offer;
    use ya_market_decentralized::protocol::{
        Discovery, OfferReceived, Propagate, StopPropagateReason,
    };
    use ya_market_decentralized::testing::OfferDao;
    use ya_market_decentralized::Offer as ModelOffer;
    use ya_market_decentralized::{MarketService, SubscriptionId};

    use serde_json::json;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Test adds offer. It should be broadcasted to other nodes in the network.
    /// Than sending unsubscribe should remove Offer from other nodes.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_broadcast_offer() -> Result<(), anyhow::Error> {
        // env_logger::init();
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
        let subscription_id = market1.subscribe_offer(&offer, identity1.clone()).await?;
        let offer = market1.matcher.get_offer(&subscription_id).await?;
        assert!(offer.is_some());

        // Expect, that Offer will appear on other nodes.
        let market2: Arc<MarketService> = network.get_market("Node-2");
        let market3: Arc<MarketService> = network.get_market("Node-3");
        wait_for_bcast(1000, &market2, &subscription_id, |o| o.is_some()).await?;
        assert_eq!(offer, market2.matcher.get_offer(&subscription_id).await?);
        assert_eq!(offer, market3.matcher.get_offer(&subscription_id).await?);

        // Unsubscribe Offer. Wait some delay for propagation.
        market1
            .unsubscribe_offer(subscription_id.clone(), identity1.clone())
            .await?;

        // Expect, that Offer will disappear on other nodes.
        wait_for_bcast(1000, &market2, &subscription_id, |o| o.is_none()).await?;
        assert!(market2.matcher.get_offer(&subscription_id).await?.is_none());
        assert!(market3.matcher.get_offer(&subscription_id).await?.is_none());

        Ok(())
    }

    /// Offer subscription id should be validated on reception. If Offer
    /// id hash doesn't match hash computed from Offer fields, Market should
    /// reject such an Offer since it could be some kind of attack.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_broadcast_offer_validation() -> Result<(), anyhow::Error> {
        // env_logger::init();
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
        let discovery2: Discovery = network.get_discovery("Node-2");
        let identity2 = network.get_default_id("Node-2");

        // Prepare Offer with subscription id changed to invalid.
        let false_subscription_id = SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53")?;
        let offer = example_offer();
        let mut offer = ModelOffer::from_new(&offer, &identity2);
        offer.id = false_subscription_id.clone();

        // Offer should be propagated to market1, but he should reject it.
        discovery2.broadcast_offer(offer).await?;
        tokio::time::delay_for(Duration::from_millis(50)).await;

        assert!(market1
            .matcher
            .get_offer(false_subscription_id.to_string())
            .await?
            .is_none());
        Ok(())
    }

    /// Nodes shouldn't broadcast unsubscribed Offers.
    /// This test broadcasts unsubscribed Offer and checks how other market Nodes
    /// behave. We expect that market nodes will stop broadcast and Discovery interface will
    /// get Offer only from himself.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_broadcast_stop_conditions() -> Result<(), anyhow::Error> {
        // env_logger::init();
        let network = MarketsNetwork::new("test_broadcast_stop_conditions")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?;

        // Add Offer on Node-1. It should be propagated to remaining nodes.
        let market1: Arc<MarketService> = network.get_market("Node-1");
        let identity1 = network.get_default_id("Node-1");

        let subscription_id = market1
            .subscribe_offer(&example_offer(), identity1.clone())
            .await?;
        let offer = market1.matcher.get_offer(&subscription_id).await?;
        assert!(offer.is_some());

        // Expect, that Offer will appear on other nodes.
        let market2: Arc<MarketService> = network.get_market("Node-2");
        wait_for_bcast(1000, &market2, &subscription_id, |o| o.is_some()).await?;
        assert_eq!(offer, market2.matcher.get_offer(&subscription_id).await?);

        // Get model Offer for direct broadcasting below.
        let db = network.get_database("Node-1");
        let model_offer = db
            .as_dao::<OfferDao>()
            .get_offer(&SubscriptionId::from_str(subscription_id.as_ref())?)
            .await?
            .unwrap();

        // Unsubscribe Offer. It should be unsubscribed on all Nodes and removed from
        // database on Node-2, since it's foreign Offer.
        market1
            .unsubscribe_offer(subscription_id.clone(), identity1.clone())
            .await?;
        assert!(market1.matcher.get_offer(&subscription_id).await?.is_none());

        // Expect, that Offer will disappear on other nodes.
        wait_for_bcast(1000, &market2, &subscription_id, |o| o.is_none()).await?;
        assert!(market2.matcher.get_offer(&subscription_id).await?.is_none());

        // Send the same Offer using Discovery interface directly.
        // Number of returning Offers will be counted.
        let offers_counter = Arc::new(AtomicUsize::new(0));
        let counter = offers_counter.clone();
        let network = network
            .add_discovery_instance(
                "Node-3",
                move |_caller: String, _msg: OfferReceived| {
                    let offers_counter = counter.clone();
                    async move {
                        offers_counter.fetch_add(1, Ordering::SeqCst);
                        Ok(Propagate::False(StopPropagateReason::AlreadyExists))
                    }
                },
                empty_on_offer_unsubscribed,
                empty_on_retrieve_offers,
            )
            .await?;

        // Broadcast already unsubscribed Offer. We will count number of Offers that will come back.
        let market3: Discovery = network.get_discovery("Node-3");
        market3.broadcast_offer(model_offer).await?;

        // Wait for Offer propagation.
        // TODO: How to wait without assuming any number of seconds?
        tokio::time::delay_for(Duration::from_millis(50)).await;

        assert_eq!(
            offers_counter.load(Ordering::SeqCst),
            1,
            "We expect to receive Offer only from ourselves"
        );
        Ok(())
    }
}
