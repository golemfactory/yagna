mod utils;

#[cfg(test)]
mod tests {
    use crate::utils::mock_node::default::*;
    use crate::utils::MarketsNetwork;

    use ya_market_decentralized::protocol::negotiation::messages::ProposalReceived;

    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    /// Test if negotiation api calls are forwarded to other instances
    /// of market on the other side of net.
    /// Since this test checks if net module in tests works correctly
    /// and doesn't have anything to do with production code, it should be removed
    /// as soon as we will have tests checking real negotiation code.
    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_negotiation_api() -> Result<(), anyhow::Error> {
        //env_logger::init();

        let proposals: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
        let proposals_clone = proposals.clone();

        let network = MarketsNetwork::new("test_negotiation_api")
            .await
            .add_provider_negotiation_api(
                "Node-1",
                empty_on_initial_proposal,
                move |_caller: String, msg: ProposalReceived| {
                    log::info!("Proposal received callback");
                    let proposals = proposals_clone.clone();
                    async move {
                        proposals.lock().await.push(msg.proposal_id.clone());
                        Ok(())
                    }
                },
                empty_on_proposal_rejected,
                empty_on_agreement_received,
                empty_on_agreement_cancelled,
            )
            .await?
            .add_requestor_negotiation_api(
                "Node-2",
                empty_on_proposal_received,
                empty_on_proposal_rejected,
                empty_on_agreement_approved,
                empty_on_agreement_rejected,
            )
            .await?;

        let negotiatior2 = network.get_requestor_negotiation_api("Node-2");
        let identity1 = network.get_default_id("Node-1");
        let identity2 = network.get_default_id("Node-2");

        let test_proposal_id = "352435685".to_string();
        negotiatior2
            .counter_proposal(identity2.identity, &test_proposal_id, identity1.identity)
            .await?;
        tokio::time::delay_for(Duration::from_millis(100)).await;

        assert_eq!(proposals.lock().await.len(), 1);
        assert_eq!(proposals.lock().await[0], test_proposal_id);
        Ok(())
    }
}
