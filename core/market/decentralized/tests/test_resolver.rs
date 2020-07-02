#[macro_use]
mod utils;

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use tokio::time::{timeout, Timeout};

    use ya_market_decentralized::testing::RawProposal;
    use ya_market_decentralized::MarketService;

    use crate::utils::{sample_client_demand, sample_client_offer, MarketStore, MarketsNetwork};
    use std::future::Future;

    /// Test adds Offer on single node. Resolver should not emit Proposal.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_single_mocked_not_resolve_offer() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_single_mocked_resolve_offer")
            .await
            .add_market_instance("Node-1")
            .await?;

        let id1 = network.get_default_id("Node-1");
        let market1: &mut MarketService = network.get_market_mut("Node-1");
        let offer = sample_client_offer();

        // when
        let subscription_id = market1.subscribe_offer(&offer, &id1).await?;
        let _offer = market1.get_offer(&subscription_id).await?;

        // then
        let proposal_rx = &mut market1.requestor_negotiation_engine.proposal_receiver;
        assert!(timeout1s(proposal_rx.recv()).await.is_err());

        Ok(())
    }

    /// Test adds Offer and Demand. Resolver should emit Proposal on Demand node.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_mocked_resolve_offer_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_single_mocked_resolve_offer")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?;

        let id1 = network.get_default_id("Node-1");
        let market1: &MarketService = network.get_market("Node-1");
        let id2 = network.get_default_id("Node-2");
        let market2: &MarketService = network.get_market("Node-2");

        // when: Add Offer on Node-1
        let offer_id = market1
            .subscribe_offer(&sample_client_offer(), &id1)
            .await?;
        // and: Add Demand on Node-2
        let demand_id = market2
            .subscribe_demand(&sample_client_demand(), &id2)
            .await?;

        // then: It should be resolved on Node-2
        let proposal_rx = &mut network
            .get_market_mut("Node-2")
            .requestor_negotiation_engine
            .proposal_receiver;
        let proposal: RawProposal = timeout1s(proposal_rx.recv()).await?.unwrap();
        assert_eq!(proposal.offer.id, offer_id);
        assert_eq!(proposal.demand.id, demand_id);

        // and: but not resolved on Node-1.
        let proposal_rx = &mut network
            .get_market_mut("Node-1")
            .requestor_negotiation_engine
            .proposal_receiver;
        assert!(timeout1s(proposal_rx.recv()).await.is_err());

        Ok(())
    }

    /// Test adds Demand on single node. Resolver should not emit Proposal.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_single_mocked_not_resolve_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_single_mocked_resolve_demand")
            .await
            .add_market_instance("Node-1")
            .await?;

        let demand = sample_client_demand();
        let id1 = network.get_default_id("Node-1");
        let market1: &mut MarketService = network.get_market_mut("Node-1");

        // when
        market1.subscribe_demand(&demand, &id1).await?;

        // then
        let proposal_rx = &mut market1.requestor_negotiation_engine.proposal_receiver;
        assert!(timeout1s(proposal_rx.recv()).await.is_err());

        Ok(())
    }

    /// Test adds Offer on two nodes and Demand third. Resolver should emit two Proposals on Demand node.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_mocked_resolve_2xoffer_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_single_mocked_resolve_offer")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?
            .add_market_instance("Node-3")
            .await?;

        let id1 = network.get_default_id("Node-1");
        let market1: &MarketService = network.get_market("Node-1");
        let id2 = network.get_default_id("Node-2");
        let market2: &MarketService = network.get_market("Node-2");
        let id3 = network.get_default_id("Node-3");
        let market3: &MarketService = network.get_market("Node-3");

        // when: Add Offer on Node-1
        let offer_id1 = market1
            .subscribe_offer(&sample_client_offer(), &id1)
            .await?;
        // when: Add Offer on Node-2
        let offer_id2 = market2
            .subscribe_offer(&sample_client_offer(), &id2)
            .await?;
        // and: Add Demand on Node-3
        let demand_id = market3
            .subscribe_demand(&sample_client_demand(), &id3)
            .await?;

        // then: It should be resolved on Node-3 two times
        let proposal_rx = &mut network
            .get_market_mut("Node-3")
            .requestor_negotiation_engine
            .proposal_receiver;
        let proposal: RawProposal = timeout1s(proposal_rx.recv()).await?.unwrap();
        assert_eq!(proposal.offer.id, offer_id1);
        assert_eq!(proposal.demand.id, demand_id);
        let proposal: RawProposal = timeout1s(proposal_rx.recv()).await?.unwrap();
        assert_eq!(proposal.offer.id, offer_id2);
        assert_eq!(proposal.demand.id, demand_id);

        // and: but not resolved on Node-1 nor Node-2.
        let proposal_rx = &mut network
            .get_market_mut("Node-1")
            .requestor_negotiation_engine
            .proposal_receiver;
        assert!(timeout1s(proposal_rx.recv()).await.is_err());
        let proposal_rx = &mut network
            .get_market_mut("Node-2")
            .requestor_negotiation_engine
            .proposal_receiver;
        assert!(timeout1s(proposal_rx.recv()).await.is_err());

        Ok(())
    }

    fn timeout1s<T: Future>(fut: T) -> Timeout<T> {
        timeout(Duration::from_secs(1), fut)
    }
}
