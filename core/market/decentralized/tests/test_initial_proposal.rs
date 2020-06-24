mod utils;

#[cfg(test)]
mod tests {
    use ya_client::model::market::event::RequestorEvent;
    use ya_client::model::market::proposal::State;
    use ya_market_decentralized::testing::QueryEventsError;
    use ya_market_decentralized::testing::SubscriptionId;
    use ya_market_decentralized::MarketService;

    use crate::utils::mock_offer::{example_demand, example_offer};
    use crate::utils::MarketsNetwork;

    use std::str::FromStr;
    use std::sync::Arc;
    use std::time::Duration;

    /// Initial proposal generated by market should be available at
    /// query events endpoint.
    /// TODO: Rewrite this test to use proposals generated by matcher instead
    ///  of injecting them negotiation engine.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_query_initial_proposal() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("test_query_initial_proposal")
            .await
            .add_market_instance("Node-1")
            .await?;

        let node1 = network.get_node("Node-1");
        let market1: Arc<MarketService> = network.get_market("Node-1");
        let (_offer_id, subscription_id) = node1
            .inject_proposal(&example_offer(), &example_demand())
            .await?;

        // We expect that proposal will be available as event.
        let events = market1
            .requestor_engine
            .query_events(&subscription_id.to_string(), 0.0, Some(5))
            .await?;

        assert_eq!(events.len(), 1);

        let proposal = match events[0].clone() {
            RequestorEvent::ProposalEvent { proposal, .. } => proposal,
            _ => panic!("Invalid event Type. ProposalEvent expected"),
        };

        assert_eq!(proposal.prev_proposal_id, None);
        assert_eq!(proposal.state()?, &State::Initial);

        // We expect that, the same event won't be available again.
        let events = market1
            .requestor_engine
            .query_events(&subscription_id.to_string(), 1.0, Some(5))
            .await?;

        assert_eq!(events.len(), 0);

        Ok(())
    }

    /// Query_events should hang on endpoint until event will come
    /// or timeout elapses.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_query_events_timeout() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("test_query_events_timeout")
            .await
            .add_market_instance("Node-1")
            .await?;

        let node1 = network.get_node("Node-1");
        let market1: Arc<MarketService> = network.get_market("Node-1");
        let identity1 = network.get_default_id("Node-1");

        let subscription_id = market1
            .subscribe_demand(&example_demand(), identity1.clone())
            .await?;
        let subscription_id = SubscriptionId::from_str(&subscription_id)?;

        let market1: Arc<MarketService> = network.get_market("Node-1");
        let demand_id = subscription_id.clone();

        // Query events, when no Proposal are in the queue yet.
        // We set timeout and we expect that function will wait until events will come.
        let query_handle = tokio::spawn(async move {
            let events = market1
                .requestor_engine
                .query_events(&subscription_id.to_string(), 1.0, Some(5))
                .await?;
            assert_eq!(events.len(), 1);
            Result::<(), anyhow::Error>::Ok(())
        });

        // Inject proposal before timeout will elapse. We expect that Proposal
        // event will be generated and query events will return it.
        tokio::time::delay_for(Duration::from_millis(500)).await;
        node1
            .inject_proposal_for_demand(&example_offer(), &demand_id)
            .await?;

        // Protect from eternal waiting.
        tokio::time::timeout(Duration::from_millis(1100), query_handle).await???;
        Ok(())
    }

    /// Query events will return before timeout will elapse, if Demand will be unsubscribed.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_query_events_unsubscribe_notification() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("test_query_events_unsubscribe_notification")
            .await
            .add_market_instance("Node-1")
            .await?;

        let market1: Arc<MarketService> = network.get_market("Node-1");
        let identity1 = network.get_default_id("Node-1");

        let subscription_id = market1
            .subscribe_demand(&example_demand(), identity1.clone())
            .await?;
        let subscription_id = SubscriptionId::from_str(&subscription_id)?;
        let demand_id = subscription_id.clone();

        // Query events, when no Proposal are in the queue yet.
        // We set timeout and we expect that function will wait until events will come.
        let query_handle = tokio::spawn(async move {
            match market1
                .requestor_engine
                .query_events(&subscription_id.to_string(), 0.5, Some(5))
                .await
            {
                Err(QueryEventsError::Unsubscribed(id)) => {
                    assert_eq!(id, subscription_id);
                }
                _ => panic!("Expected unsubscribed error."),
            }
            Result::<(), anyhow::Error>::Ok(())
        });

        // Unsubscribe Demand. query_events should return with unsubscribed error.
        tokio::time::delay_for(Duration::from_millis(200)).await;

        let market1: Arc<MarketService> = network.get_market("Node-1");
        market1
            .unsubscribe_demand(demand_id.to_string(), identity1.clone())
            .await?;

        // Protect from eternal waiting.
        tokio::time::timeout(Duration::from_millis(700), query_handle).await???;

        Ok(())
    }

    /// Tests if query events returns proper error on invalid input
    /// or unsubscribed demand.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_query_events_edge_cases() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("test_query_events_edge_cases")
            .await
            .add_market_instance("Node-1")
            .await?;

        let node1 = network.get_node("Node-1");
        let market1: Arc<MarketService> = network.get_market("Node-1");
        let identity1 = network.get_default_id("Node-1");

        let (_offer_id, demand_id) = node1
            .inject_proposal(&example_offer(), &example_demand())
            .await?;

        // We should reject calls with negative maxEvents.
        match market1
            .requestor_engine
            .query_events(&demand_id.to_string(), 0.0, Some(-5))
            .await
        {
            Err(QueryEventsError::InvalidMaxEvents(value)) => {
                assert_eq!(value, -5);
            }
            _ => panic!("Negative maxEvents - expected error"),
        };

        // Negative timeout should be treated as immediate checking events and return.
        let events = market1
            .requestor_engine
            .query_events(&demand_id.to_string(), -5.0, None)
            .await?;
        assert_eq!(events.len(), 1);

        // Restore available Proposal
        let (_offer_id, demand_id) = node1
            .inject_proposal(&example_offer(), &example_demand())
            .await?;

        // maxEvents equal to 0 isn't forbidden value, but should return 0 events,
        // even if they exist.
        let events = market1
            .requestor_engine
            .query_events(&demand_id.to_string(), 1.0, Some(0))
            .await?;
        assert_eq!(events.len(), 0);

        // Query events returns error, if Demand was unsubscribed.
        market1
            .unsubscribe_demand(demand_id.to_string(), identity1.clone())
            .await?;

        match market1
            .requestor_engine
            .query_events(&demand_id.to_string(), 0.0, None)
            .await
        {
            Err(QueryEventsError::Unsubscribed(subscription_id)) => {
                assert_eq!(&subscription_id, &demand_id);
            }
            _ => panic!("Expected Unsubscribed error."),
        }

        Ok(())
    }
}
