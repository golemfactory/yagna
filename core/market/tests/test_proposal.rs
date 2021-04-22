use ya_market::testing::{
    bcast::assert_offers_broadcasted,
    mock_offer::client::{sample_demand, sample_offer},
    proposal_util::exchange_draft_proposals,
    GetProposalError, MarketServiceExt, MarketsNetwork, Owner, ProposalError,
};
use ya_market::{assert_err_eq, MarketService};

use std::sync::Arc;
use tokio::time::Duration;
use ya_client::model::market::proposal::State;
use ya_client::model::market::RequestorEvent;
use ya_core_model::NodeId;

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_get_proposal() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Requestor1")
        .await
        .add_market_instance("Provider1")
        .await;

    let req_market = network.get_market("Requestor1");
    let prov_market = network.get_market("Provider1");
    let prov_id = network.get_default_id("Provider1");

    // Requestor side
    let proposal_id = exchange_draft_proposals(&network, "Requestor1", "Provider1")
        .await
        .unwrap()
        .proposal_id;
    let result = req_market.get_proposal(&proposal_id).await;

    assert!(result.is_ok());
    let proposal = result.unwrap().into_client().unwrap();

    assert_eq!(proposal.state, State::Draft);
    assert_eq!(proposal.proposal_id, proposal_id.to_string());
    assert_eq!(proposal.issuer_id, prov_id.identity);
    assert!(proposal.prev_proposal_id().is_ok());

    // Provider side
    let proposal_id = proposal_id.translate(Owner::Provider);
    let result = prov_market.get_proposal(&proposal_id).await;

    assert!(result.is_ok());
    let proposal = result.unwrap().into_client().unwrap();

    assert_eq!(proposal.state, State::Draft);
    assert_eq!(proposal.proposal_id, proposal_id.to_string());
    assert_eq!(proposal.issuer_id, prov_id.identity);
    assert!(proposal.prev_proposal_id().is_ok());
}

/// Try to query not existing Proposal.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_get_proposal_not_found() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Requestor1")
        .await
        .add_market_instance("Provider1")
        .await;

    let req_market = network.get_market("Requestor1");

    // Create some Proposals, that will be unused.
    exchange_draft_proposals(&network, "Requestor1", "Provider1")
        .await
        .unwrap();

    // Invalid id
    let proposal_id = "P-0000000000000000000000000000000000000000000000000000000000000003"
        .parse()
        .unwrap();
    let result = req_market.get_proposal(&proposal_id).await;

    assert!(result.is_err());
    assert_err_eq!(
        ProposalError::Get(GetProposalError::NotFound(proposal_id, None)),
        result
    );
}

/// We don't want to give advantage for the oldest Offers, so we should shuffle
/// results of `collect_offers` endpoint.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_proposal_random_shuffle() {
    let mut network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await;

    let num = 10;
    for i in 0..num {
        network = network
            .add_market_instance(&format!("Provider-{}", i))
            .await;
    }

    let market1 = network.get_market("Node-1");
    let identity1 = network.get_default_id("Node-1");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await
        .unwrap();

    // Wait between subscribing Offers. Thanks to this, Offers
    // will be propagated and added to queue in order.
    let mut offers = vec![];
    let mut ids = vec![];
    let mut markets: Vec<Arc<MarketService>> = vec![];
    for i in 0..num {
        let node_name = format!("Provider-{}", i);
        let market = network.get_market(&node_name);
        markets.push(market.clone());
        let identity = network.get_default_id(&node_name);

        offers.push(
            market
                .subscribe_offer(&sample_offer(), &identity)
                .await
                .unwrap(),
        );
        ids.push(identity.identity);

        tokio::time::delay_for(Duration::from_millis(200)).await;
    }

    let markets: Vec<&MarketService> = markets.iter().map(|m| m.as_ref()).collect();
    assert_offers_broadcasted(&markets[..], offers.iter()).await;

    let events = market1
        .query_events(&demand_id, 1.2, Some(num + 4))
        .await
        .unwrap();
    assert_eq!(events.len(), num as usize);

    let incoming_ids = events
        .into_iter()
        .filter_map(|event| match event {
            RequestorEvent::ProposalEvent { proposal, .. } => Some(proposal.issuer_id),
            _ => None,
        })
        .collect::<Vec<NodeId>>();

    // If proposals were really shuffled, we expect incoming order to be different
    // from initialization order.
    assert_eq!(incoming_ids.len(), ids.len());
    assert_ne!(incoming_ids, ids);
}
