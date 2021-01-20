use ya_client::model::market::proposal::State;
use ya_market::testing::events_helper::{provider, requestor, ClientProposalHelper};
use ya_market::testing::mock_offer::client::{
    not_matching_demand, not_matching_offer, sample_demand, sample_offer,
};
use ya_market::testing::proposal_util::{exchange_draft_proposals, NegotiationHelper};
use ya_market::testing::MarketsNetwork;
use ya_market::testing::Owner;
use ya_market::testing::{ProposalError, ProposalValidationError, SaveProposalError};
use ya_market_resolver::flatten::flatten_json;

/// Test countering initial and draft proposals on both Provider and Requestor side.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_exchanging_draft_proposals() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await
        .unwrap();
    let offer = sample_offer();
    let offer_id = market2.subscribe_offer(&offer, &identity2).await.unwrap();

    // Expect events generated on requestor market.
    let proposal0 = requestor::query_proposal(&market1, &demand_id, "Initial #R")
        .await
        .unwrap();
    assert_eq!(
        proposal0.properties,
        flatten_json(&offer.properties).unwrap()
    );
    assert_eq!(proposal0.constraints, offer.constraints);
    assert_eq!(proposal0.issuer_id, identity2.identity);
    assert_eq!(proposal0.state, State::Initial);
    assert_eq!(proposal0.prev_proposal_id, None);

    // Requestor counters initial proposal. We expect that provider will get proposal event.
    let proposal1_req = sample_demand();
    let proposal1_req_id = market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id().unwrap(),
            &proposal1_req,
            &identity1,
        )
        .await
        .unwrap();

    // Provider receives Proposal
    let proposal1_prov = provider::query_proposal(&market2, &offer_id, "Initial #P")
        .await
        .unwrap();
    let proposal1_prov_id = proposal1_req_id.clone().translate(Owner::Provider);

    assert_eq!(proposal1_prov.constraints, proposal1_req.constraints);
    assert_eq!(
        proposal1_prov.properties,
        flatten_json(&proposal1_req.properties).unwrap()
    );
    assert_eq!(proposal1_prov.proposal_id, proposal1_prov_id.to_string());
    assert_eq!(proposal1_prov.issuer_id, identity1.identity);
    assert_eq!(proposal1_prov.state, State::Draft);
    // Requestor and Provider have different first Proposals IDs
    assert!(proposal1_prov.prev_proposal_id.is_some());

    // Provider counters proposal.
    let proposal2_prov = sample_offer();
    let proposal2_id = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal1_prov_id, &proposal2_prov, &identity2)
        .await
        .unwrap();

    // Requestor receives proposal.
    let proposal2_req = requestor::query_proposal(&market1, &demand_id, "Counter #R1")
        .await
        .unwrap();
    let proposal2_req_id = proposal2_id.clone().translate(Owner::Requestor);

    assert_eq!(proposal2_req.constraints, proposal2_prov.constraints);
    assert_eq!(
        proposal2_req.properties,
        flatten_json(&proposal2_prov.properties).unwrap()
    );
    assert_eq!(proposal2_req.proposal_id, proposal2_req_id.to_string());
    assert_eq!(proposal2_req.issuer_id, identity2.identity);
    assert_eq!(proposal2_req.state, State::Draft);
    assert_eq!(
        proposal2_req.prev_proposal_id,
        Some(proposal1_prov_id.translate(Owner::Requestor).to_string()),
    );

    // Requestor counters draft proposal.
    let proposal3_req = sample_demand();
    let proposal3_req_id = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal2_req_id, &proposal3_req, &identity1)
        .await
        .unwrap();

    // Provider receives Proposal
    let proposal3_prov = provider::query_proposal(&market2, &offer_id, "Counter #P1")
        .await
        .unwrap();
    let proposal3_prov_id = proposal3_req_id.clone().translate(Owner::Provider);

    assert_eq!(proposal3_prov.constraints, proposal3_req.constraints);
    assert_eq!(
        proposal3_prov.properties,
        flatten_json(&proposal3_req.properties).unwrap()
    );
    assert_eq!(proposal3_prov.proposal_id, proposal3_prov_id.to_string());
    assert_eq!(proposal3_prov.issuer_id, identity1.identity);
    assert_eq!(proposal3_prov.state, State::Draft);
    assert_eq!(
        proposal3_prov.prev_proposal_id,
        Some(proposal2_req_id.translate(Owner::Provider).to_string()),
    );
}

/// Can't counter proposal, that was already countered.
/// Market should reject such attempts.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_counter_countered_proposal() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await
        .unwrap();
    let offer_id = market2
        .subscribe_offer(&sample_offer(), &identity2)
        .await
        .unwrap();

    // REQUESTOR side.
    let proposal0 = requestor::query_proposal(&market1, &demand_id, "Initial #R")
        .await
        .unwrap();
    let proposal0_id = proposal0.get_proposal_id().unwrap();

    // Counter proposal for the first time.
    let proposal1 = sample_demand();
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await
        .unwrap();

    // Now counter proposal for the second time. It should fail.
    let proposal2 = sample_demand();
    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal2, &identity1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Save(SaveProposalError::AlreadyCountered(id)) => {
            assert_eq!(id, proposal0_id);
        }
        e => panic!("Expected AlreadyCountered error, got: {}", e),
    }

    // PROVIDER side
    let proposal0 = provider::query_proposal(&market2, &offer_id, "Initial #P")
        .await
        .unwrap();
    let proposal0_id = proposal0.get_proposal_id().unwrap();

    // Counter proposal for the first time.
    let proposal1 = sample_offer();
    market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal0_id, &proposal1, &identity2)
        .await
        .unwrap();

    // Now counter proposal for the second time. It should fail.
    let proposal2 = sample_offer();
    let result = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal0_id, &proposal2, &identity2)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Save(SaveProposalError::AlreadyCountered(id)) => {
            assert_eq!(id, proposal0_id)
        }
        e => panic!("Expected AlreadyCountered error, got: {}", e),
    }
}

/// Can't counter own proposal.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_counter_own_proposal() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await
        .unwrap();
    let offer_id = market2
        .subscribe_offer(&sample_offer(), &identity2)
        .await
        .unwrap();

    // REQUESTOR side.
    let proposal0 = requestor::query_proposal(&market1, &demand_id, "Initial #R")
        .await
        .unwrap();

    let proposal1 = sample_demand();
    let proposal1_id = market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id().unwrap(),
            &proposal1,
            &identity1,
        )
        .await
        .unwrap();

    // Counter proposal1, that was created by us.
    let proposal2 = sample_demand();

    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal1_id, &proposal2, &identity1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Validation(ProposalValidationError::OwnProposal(id)) => {
            assert_eq!(id, proposal1_id)
        }
        e => panic!("Expected ProposalValidationError::OwnProposal, got: {}", e),
    }

    // PROVIDER side
    let proposal0 = provider::query_proposal(&market2, &offer_id, "Initial #P")
        .await
        .unwrap();

    let proposal1 = sample_offer();
    let proposal1_id = market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal0.get_proposal_id().unwrap(),
            &proposal1,
            &identity2,
        )
        .await
        .unwrap();

    // Counter proposal1, that was created by us.
    let proposal2 = sample_offer();

    let result = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal1_id, &proposal2, &identity2)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Validation(ProposalValidationError::OwnProposal(id)) => {
            assert_eq!(id, proposal1_id)
        }
        e => panic!("Expected ProposalValidationError::OwnProposal, got: {}", e),
    }
}

/// Requestor can't counter Proposal, for which he has unsubscribed Demand.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_counter_unsubscribed_demand() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await
        .unwrap();
    market2
        .subscribe_offer(&sample_offer(), &identity2)
        .await
        .unwrap();

    let proposal0 = requestor::query_proposal(&market1, &demand_id, "Initial #R")
        .await
        .unwrap();
    market1
        .unsubscribe_demand(&demand_id, &identity1)
        .await
        .unwrap();

    let proposal1 = sample_demand();
    let result = market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id().unwrap(),
            &proposal1,
            &identity1,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Validation(ProposalValidationError::NoSubscription(id)) => {
            assert_eq!(id, demand_id)
        }
        e => panic!(
            "Expected ProposalValidationError::NoSubscription, got: {}",
            e
        ),
    }
}

/// Provider can't counter Proposal, for which he has unsubscribed Offer.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_counter_unsubscribed_offer() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await
        .unwrap();
    let offer_id = market2
        .subscribe_offer(&sample_offer(), &identity2)
        .await
        .unwrap();

    let proposal0 = requestor::query_proposal(&market1, &demand_id, "Initial #R")
        .await
        .unwrap();
    let proposal1 = sample_demand();
    market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id().unwrap(),
            &proposal1,
            &identity1,
        )
        .await
        .unwrap();

    // PROVIDER side
    let proposal0 = provider::query_proposal(&market2, &offer_id, "Initial #P")
        .await
        .unwrap();
    market2
        .unsubscribe_offer(&offer_id, &identity2)
        .await
        .unwrap();

    let proposal1 = sample_offer();
    let result = market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal0.get_proposal_id().unwrap(),
            &proposal1,
            &identity2,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Validation(ProposalValidationError::Unsubscribed(id)) => {
            assert_eq!(id, offer_id)
        }
        e => panic!("Expected ProposalValidationError::Unsubscribed, got: {}", e),
    }
}

/// Requestor tries to counter initial Proposal, for which Offer was unsubscribed on remote Node.
/// Negotiation attempt should be rejected by Provider Node.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_counter_initial_unsubscribed_remote_offer() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await
        .unwrap();
    let offer_id = market2
        .subscribe_offer(&sample_offer(), &identity2)
        .await
        .unwrap();

    let proposal0 = requestor::query_proposal(&market1, &demand_id, "Initial #R")
        .await
        .unwrap();

    // When we will counter this Proposal, Provider will have it already unsubscribed.
    market2
        .unsubscribe_offer(&offer_id, &identity2)
        .await
        .unwrap();

    let proposal1 = sample_demand();
    let result = market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id().unwrap(),
            &proposal1,
            &identity1,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Validation(ProposalValidationError::Unsubscribed(id)) => {
            assert_eq!(id, offer_id)
        }
        e => panic!("Expected ProposalValidationError::Unsubscribed, got: {}", e),
    }
}

/// Requestor tries to counter draft Proposal, for which Offer was unsubscribed on remote Node.
/// Negotiation attempt should be rejected by Provider Node.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_counter_draft_unsubscribed_remote_offer() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let NegotiationHelper {
        proposal_id: proposal0_id,
        offer_id,
        demand_id,
        ..
    } = exchange_draft_proposals(&network, "Node-1", "Node-2")
        .await
        .unwrap();

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");
    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    // When we will counter this Proposal, Provider will have it already unsubscribed.
    market2
        .unsubscribe_offer(&offer_id, &identity2)
        .await
        .unwrap();

    let proposal1 = sample_demand();
    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Validation(ProposalValidationError::Unsubscribed(id)) => {
            assert_eq!(id, offer_id)
        }
        e => panic!("Expected ProposalValidationError::Unsubscribed, got: {}", e),
    }
}

/// Provider tries to counter draft Proposal, for which Demand was unsubscribed on remote Node.
/// Negotiation attempt should be rejected by Requestor Node.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_counter_draft_unsubscribed_remote_demand() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let NegotiationHelper {
        proposal_id: proposal0_id,
        offer_id,
        demand_id,
        ..
    } = exchange_draft_proposals(&network, "Node-1", "Node-2")
        .await
        .unwrap();

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");
    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let proposal1 = sample_demand();
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await
        .unwrap();

    let proposal2 = provider::query_proposal(&market2, &offer_id, "Counter #P")
        .await
        .unwrap();
    market1
        .unsubscribe_demand(&demand_id, &identity1)
        .await
        .unwrap();

    let proposal3 = sample_offer();
    let result = market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal2.get_proposal_id().unwrap(),
            &proposal3,
            &identity2,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Send(..) => (),
        e => panic!("Expected ProposalError::Send, got: {}", e),
    }
}

/// Try to send not matching counter Proposal to Provider. Our market
/// should reject such Proposal. Error should occur on Requestor side.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_not_matching_counter_demand() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let NegotiationHelper {
        proposal_id: proposal0_id,
        demand_id,
        ..
    } = exchange_draft_proposals(&network, "Node-1", "Node-2")
        .await
        .unwrap();

    let market1 = network.get_market("Node-1");
    let identity1 = network.get_default_id("Node-1");
    let proposal1 = not_matching_demand();
    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Validation(ProposalValidationError::NotMatching(..)) => (),
        e => panic!("Expected ProposalValidationError::NotMatching, got: {}", e),
    }
}

/// Try to send not matching counter Proposal to Requestor. Our market
/// should reject such Proposal. Error should occur on Provider side.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_not_matching_counter_offer() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await;

    let NegotiationHelper {
        proposal_id: proposal0_id,
        demand_id,
        offer_id,
        ..
    } = exchange_draft_proposals(&network, "Node-1", "Node-2")
        .await
        .unwrap();

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");
    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let proposal1 = sample_demand();
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await
        .unwrap();

    let proposal2 = provider::query_proposal(&market2, &offer_id, "Counter #P")
        .await
        .unwrap();
    let proposal3 = not_matching_offer();
    let result = market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal2.get_proposal_id().unwrap(),
            &proposal3,
            &identity2,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Validation(ProposalValidationError::NotMatching(..)) => (),
        e => panic!("Expected ProposalValidationError::NotMatching, got: {}", e),
    }
}

/// Negotiations between Provider and Requestor using the same Identity
/// (which means that they are on the same node) is forbidden. Matcher should reject
/// such Offer-Demand pairs.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_reject_negotiations_same_identity() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await;

    let market1 = network.get_market("Node-1");
    let identity1 = network.get_default_id("Node-1");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await
        .unwrap();
    market1
        .subscribe_offer(&sample_offer(), &identity1)
        .await
        .unwrap();

    // We expect, that there will be no Proposals.
    let events = market1
        .requestor_engine
        .query_events(&demand_id, 3.0, Some(5))
        .await
        .unwrap();
    assert_eq!(events.len(), 0);
}
