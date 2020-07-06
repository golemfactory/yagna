#[macro_use]
mod utils;

#[cfg(test)]
mod tests {
    use crate::utils::{
        client::{sample_demand, sample_offer},
        MarketServiceExt, MarketsNetwork,
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
        let offer = sample_offer();

        // when
        let offer_id = provider_mkt.subscribe_offer(&offer, &id1).await?;
        let _offer = provider_mkt.get_offer(&offer_id).await?;

        // TODO // then
        // let events = provider_mkt.query_events(&offer_id, 0.2, None).await?;
        // assert_eq!(events.len(), 0);

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
            .add_market_instance("Provider-1")
            .await?
            .add_market_instance("Requestor-1")
            .await?;

        let id1 = network.get_default_id("Provider-1");
        let provider_mkt = network.get_market("Provider-1");
        let id2 = network.get_default_id("Requestor-1");
        let requestor_mkt = network.get_market("Requestor-1");

        // when: Add Offer on Provider
        let _offer_id = provider_mkt.subscribe_offer(&sample_offer(), &id1).await?;
        // and: Add Demand on Requestor
        let demand_id = requestor_mkt
            .subscribe_demand(&sample_demand(), &id2)
            .await?;

        // then: It should be resolved on Requestor
        let events = requestor_mkt.query_events(&demand_id, 0.2, None).await?;
        assert_eq!(events.len(), 1);

        // TODO // and: but not resolved on Provider.
        // let events = provider_mkt.query_events(&offer_id, 0.2, None).await?;
        // assert_eq!(events.len(), 0);

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

        let demand = sample_demand();
        let id1 = network.get_default_id("Node-1");
        let requestor_mkt = network.get_market("Node-1");

        // when
        let demand_id = requestor_mkt.subscribe_demand(&demand, &id1).await?;

        // then
        let events = requestor_mkt.query_events(&demand_id, 0.2, None).await?;
        assert_eq!(events.len(), 0);

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
            .add_market_instance("Provider-1")
            .await?
            .add_market_instance("Provider-2")
            .await?
            .add_market_instance("Requestor-1")
            .await?;

        let id1 = network.get_default_id("Provider-1");
        let provider_mkt1 = network.get_market("Provider-1");
        let id2 = network.get_default_id("Provider-2");
        let provider_mkt2 = network.get_market("Provider-2");
        let id3 = network.get_default_id("Requestor-1");
        let requestor_mkt = network.get_market("Requestor-1");

        // when: Add Offer on Provider-1
        let _offer_id1 = provider_mkt1.subscribe_offer(&sample_offer(), &id1).await?;
        // when: Add Offer on Provider-2
        let _offer_id2 = provider_mkt2.subscribe_offer(&sample_offer(), &id2).await?;
        // and: Add Demand on Requestor
        let demand_id = requestor_mkt
            .subscribe_demand(&sample_demand(), &id3)
            .await?;

        // then: It should be resolved on Requestor two times
        let events = requestor_mkt.query_events(&demand_id, 0.2, None).await?;
        assert_eq!(events.len(), 2);
        // TODO // and: but not resolved on Provider-1
        // let events = provider_mkt1.query_events(&offer_id1, 0.2, None).await?;
        // assert_eq!(events.len(), 0);
        // // and: not on Provider-2.
        // let events = provider_mkt2.query_events(&offer_id2, 0.2, None).await?;
        // assert_eq!(events.len(), 0);

        Ok(())
    }
}
