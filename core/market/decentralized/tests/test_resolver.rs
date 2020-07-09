#[macro_use]
mod utils;

#[cfg(test)]
mod tests {
    use std::{future::Future, time::Duration};
    use tokio::time::{timeout, Timeout};

    use crate::utils::{
        client::{sample_demand, sample_offer},
        MarketsNetwork,
    };

    /// Test adds Offer on single node. Resolver should not emit Proposal.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_single_not_resolve_offer() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_single_not_resolve_offer")
            .await
            .add_matcher_instance("Node-1")
            .await?;

        let id1 = network.get_default_id("Node-1");
        let provider = network.get_matcher("Node-1");
        let offer = sample_offer();

        // when
        let _offer = provider.subscribe_offer(&offer, &id1).await?;

        // then
        let listener = network.get_event_listeners("Node-1");
        assert!(timeout200ms(listener.proposal_receiver.recv())
            .await
            .is_err());

        Ok(())
    }

    /// Test adds Offer and Demand. Resolver should emit Proposal on Demand node.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_resolve_offer_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_resolve_offer_demand")
            .await
            .add_matcher_instance("Provider-1")
            .await?
            .add_matcher_instance("Requestor-1")
            .await?;

        let id1 = network.get_default_id("Provider-1");
        let provider = network.get_matcher("Provider-1");
        let id2 = network.get_default_id("Requestor-1");
        let requestor = network.get_matcher("Requestor-1");

        // when: Add Offer on Provider
        let offer = provider.subscribe_offer(&sample_offer(), &id1).await?;
        // and: Add Demand on Requestor
        let demand = requestor.subscribe_demand(&sample_demand(), &id2).await?;

        // then: It should be resolved on Requestor
        let listener = network.get_event_listeners("Requestor-1");
        let proposal = timeout200ms(listener.proposal_receiver.recv())
            .await?
            .unwrap();
        assert_eq!(proposal.offer, offer);
        assert_eq!(proposal.demand, demand);

        // and: but not resolved on Provider.
        let listener = network.get_event_listeners("Provider-1");
        assert!(timeout200ms(listener.proposal_receiver.recv())
            .await
            .is_err());

        Ok(())
    }

    /// Test adds Demand on single node. Resolver should not emit Proposal.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_single_not_resolve_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_single_not_resolve_demand")
            .await
            .add_matcher_instance("Node-1")
            .await?;

        let demand = sample_demand();
        let id1 = network.get_default_id("Node-1");
        let requestor = network.get_matcher("Node-1");

        // when
        let _demand = requestor.subscribe_demand(&demand, &id1).await?;

        // then
        let listener = network.get_event_listeners("Node-1");
        assert!(timeout200ms(listener.proposal_receiver.recv())
            .await
            .is_err());

        Ok(())
    }

    /// Test adds Offer on two nodes and Demand third. Resolver should emit two Proposals on Demand node.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_resolve_2xoffer_demand() -> Result<(), anyhow::Error> {
        // given
        let _ = env_logger::builder().try_init();
        let mut network = MarketsNetwork::new("test_resolve_2xoffer_demand")
            .await
            .add_matcher_instance("Provider-1")
            .await?
            .add_matcher_instance("Provider-2")
            .await?
            .add_matcher_instance("Requestor-1")
            .await?;

        let id1 = network.get_default_id("Provider-1");
        let provider1 = network.get_matcher("Provider-1");
        let id2 = network.get_default_id("Provider-2");
        let provider2 = network.get_matcher("Provider-2");
        let id3 = network.get_default_id("Requestor-1");
        let requestor = network.get_matcher("Requestor-1");

        // when: Add Offer on Provider-1
        let offer1 = provider1.subscribe_offer(&sample_offer(), &id1).await?;
        // when: Add Offer on Provider-2
        let offer2 = provider2.subscribe_offer(&sample_offer(), &id2).await?;
        // and: Add Demand on Requestor
        let demand = requestor.subscribe_demand(&sample_demand(), &id3).await?;

        // then: It should be resolved on Requestor two times
        let listener = network.get_event_listeners("Requestor-1");
        let proposal = timeout200ms(listener.proposal_receiver.recv())
            .await?
            .unwrap();
        assert_eq!(proposal.offer, offer1);
        assert_eq!(proposal.demand, demand);
        let proposal = timeout200ms(listener.proposal_receiver.recv())
            .await?
            .unwrap();
        assert_eq!(proposal.offer, offer2);
        assert_eq!(proposal.demand, demand);

        // and: but not resolved on Provider-1
        let listener = network.get_event_listeners("Provider-1");
        assert!(timeout200ms(listener.proposal_receiver.recv())
            .await
            .is_err());
        // and: not on Provider-2.
        let listener = network.get_event_listeners("Provider-2");
        assert!(timeout200ms(listener.proposal_receiver.recv())
            .await
            .is_err());

        Ok(())
    }

    fn timeout200ms<T: Future>(fut: T) -> Timeout<T> {
        timeout(Duration::from_millis(200), fut)
    }
}
