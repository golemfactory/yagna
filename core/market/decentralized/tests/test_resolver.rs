#[macro_use]
mod utils;

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use tokio::time::{timeout, Timeout};

    use ya_client::model::market::Proposal;
    use ya_market_decentralized::MarketService;

    use crate::utils::{sample_client_demand, sample_client_offer, MarketStore, MarketsNetwork};
    use std::future::Future;

    /// Test adds Offer on single node. Mocked Resolver should emit dummy Proposal.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_single_mocked_resolve_offer() -> Result<(), anyhow::Error> {
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
        let proposal: Proposal = timeout1s(proposal_rx.recv()).await?.unwrap();

        assert_eq!(proposal.proposal_id, Some(subscription_id.to_string()));
        assert_eq!(proposal.properties, offer.properties);
        assert_eq!(proposal.constraints, offer.constraints);

        Ok(())
    }

    /// Test adds Offer. Mocked Resolver should emit dummy Proposal on every node.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_mocked_resolve_offer() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_single_mocked_resolve_offer")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?;

        let id1 = network.get_default_id("Node-1");
        let market1: &mut MarketService = network.get_market_mut("Node-1");

        // when: Add Offer on Node-1
        let subscription_id = market1
            .subscribe_offer(&sample_client_offer(), &id1)
            .await?;

        // then: It should be resolved locally
        let proposal_rx = &mut market1.requestor_negotiation_engine.proposal_receiver;
        let proposal: Proposal = timeout1s(proposal_rx.recv()).await?.unwrap();
        assert_eq!(proposal.proposal_id, Some(subscription_id.to_string()));

        // and also: propagated to Node-2 and resolved.
        let market2: &mut MarketService = network.get_market_mut("Node-2");
        let proposal_rx = &mut market2.requestor_negotiation_engine.proposal_receiver;
        let proposal: Proposal = timeout1s(proposal_rx.recv()).await?.unwrap();
        assert_eq!(proposal.proposal_id, Some(subscription_id.to_string()));

        Ok(())
    }

    /// Test adds Demand on single node. Mocked Resolver should emit dummy Proposal.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_single_mocked_resolve_demand() -> Result<(), anyhow::Error> {
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
        let subscription_id = market1.subscribe_demand(&demand, &id1).await?;

        // then
        let proposal_rx = &mut market1.requestor_negotiation_engine.proposal_receiver;
        let proposal: Proposal = timeout1s(proposal_rx.recv()).await?.unwrap();

        assert_eq!(proposal.proposal_id, Some(subscription_id.to_string()));
        assert_eq!(proposal.properties, demand.properties);
        assert_eq!(proposal.constraints, demand.constraints);

        Ok(())
    }

    /// Test adds Demand. Mocked Resolver should emit dummy Proposal just on first Node.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_mocked_resolve_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_mocked_resolve_demand")
            .await
            .add_market_instance("Node-1")
            .await?
            .add_market_instance("Node-2")
            .await?;

        let id1 = network.get_default_id("Node-1");
        let market1: &mut MarketService = network.get_market_mut("Node-1");

        // when: Add Demand on Node-1
        let subscription_id = market1
            .subscribe_demand(&sample_client_demand(), &id1)
            .await?;

        // then: It should be resolved locally
        let proposal_rx = &mut market1.requestor_negotiation_engine.proposal_receiver;
        let proposal: Proposal = timeout1s(proposal_rx.recv()).await?.unwrap();
        assert_eq!(proposal.proposal_id, Some(subscription_id.to_string()));

        // and also: propagated to Node-2 and resolved.
        let market2: &mut MarketService = network.get_market_mut("Node-2");

        let proposal_rx = &mut market2.requestor_negotiation_engine.proposal_receiver;
        assert!(timeout1s(proposal_rx.recv()).await.is_err());

        Ok(())
    }

    fn timeout1s<T: Future>(fut: T) -> Timeout<T> {
        timeout(Duration::from_secs(1), fut)
    }
}
