#[macro_use]
mod utils;

#[cfg(test)]
mod tests {
    use crate::utils::{
        sample_client_demand, sample_client_offer, MarketServiceExt, MarketsNetwork,
    };

    /// Test adds Offer on single node. Resolver should not emit Proposal.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_single_not_resolve_offer() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let network = MarketsNetwork::new("test_single_resolve_offer")
            .await
            .add_market_instance("Node-1")
            .await?;

        let id1 = network.get_default_id("Node-1");
        let provider_mkt = network.get_market("Node-1");
        let offer = sample_client_offer();

        // when
        let offer_id = provider_mkt.subscribe_offer(&offer, &id1).await?;
        let _offer = provider_mkt.get_offer(&offer_id).await?;

        // then
        // TODO: spawn and wait
        assert!(provider_mkt.wait_1s_for_event(&offer_id).await.is_err());

        Ok(())
    }

    /// Test adds Offer and Demand. Resolver should emit Proposal on Demand node.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_resolve_offer_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let network = MarketsNetwork::new("test_resolve_offer_demand")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?;

        let id1 = network.get_default_id("Node-1");
        let provider_mkt = network.get_market("Node-1");
        let id2 = network.get_default_id("Node-2");
        let requestor_mkt = network.get_market("Node-2");

        // when: Add Offer on Node-1
        let offer_id = provider_mkt
            .subscribe_offer(&sample_client_offer(), &id1)
            .await?;
        // and: Add Demand on Node-2
        let demand_id = requestor_mkt
            .subscribe_demand(&sample_client_demand(), &id2)
            .await?;

        // then: It should be resolved on Node-2
        assert!(requestor_mkt.wait_1s_for_event(&demand_id).await.is_ok());

        // and: but not resolved on Node-1.
        assert!(provider_mkt.wait_1s_for_event(&offer_id).await.is_err());

        Ok(())
    }

    /// Test adds Demand on single node. Resolver should not emit Proposal.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_single_not_resolve_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let network = MarketsNetwork::new("test_single_not_resolve_demand")
            .await
            .add_market_instance("Node-1")
            .await?;

        let demand = sample_client_demand();
        let id1 = network.get_default_id("Node-1");
        let requestor_mkt = network.get_market("Node-1");

        // when
        let demand_id = requestor_mkt.subscribe_demand(&demand, &id1).await?;

        // then
        assert!(requestor_mkt.wait_1s_for_event(&demand_id).await.is_err());

        Ok(())
    }

    /// Test adds Offer on two nodes and Demand third. Resolver should emit two Proposals on Demand node.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_resolve_2xoffer_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let network = MarketsNetwork::new("test_resolve_2xoffer_demand")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?
            .add_market_instance("Node-3")
            .await?;

        let id1 = network.get_default_id("Node-1");
        let provider_mkt1 = network.get_market("Node-1");
        let id2 = network.get_default_id("Node-2");
        let provider_mkt2 = network.get_market("Node-2");
        let id3 = network.get_default_id("Node-3");
        let requestor_mkt3 = network.get_market("Node-3");

        // when: Add Offer on Node-1
        let offer_id1 = provider_mkt1
            .subscribe_offer(&sample_client_offer(), &id1)
            .await?;
        // when: Add Offer on Node-2
        let offer_id2 = provider_mkt2
            .subscribe_offer(&sample_client_offer(), &id2)
            .await?;
        // and: Add Demand on Node-3
        let demand_id = requestor_mkt3
            .subscribe_demand(&sample_client_demand(), &id3)
            .await?;

        // then: It should be resolved on Node-3 two times
        assert!(requestor_mkt3.wait_1s_for_event(&demand_id).await.is_ok());
        assert!(requestor_mkt3.wait_1s_for_event(&demand_id).await.is_ok());

        // and: but not resolved on Node-1 nor Node-2.
        assert!(provider_mkt1.wait_1s_for_event(&offer_id1).await.is_err());
        assert!(provider_mkt2.wait_1s_for_event(&offer_id2).await.is_err());

        Ok(())
    }
}
