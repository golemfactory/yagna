use ya_client::model::market::{proposal::State, RequestorEvent};
use ya_market::testing::{
    events_helper::{provider, requestor, ClientProposalHelper},
    mock_node::assert_offers_broadcasted,
    mock_offer::client::{not_matching_demand, not_matching_offer, sample_demand, sample_offer},
    mock_offer::flatten_json,
    negotiation::error::{CounterProposalError, RemoteProposalError},
    proposal_util::{exchange_draft_proposals, NegotiationHelper},
    MarketServiceExt, MarketsNetwork, Owner, ProposalError, ProposalState, ProposalValidationError,
    SaveProposalError,
};

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
    assert_eq!(proposal0.properties, flatten_json(&offer.properties));
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
        flatten_json(&proposal1_req.properties)
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
        flatten_json(&proposal2_prov.properties)
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
        flatten_json(&proposal3_req.properties)
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
    let proposal0_id = proposal0.get_proposal_id().unwrap();

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
        ProposalError::Send(
            proposal_id_send,
            CounterProposalError::Remote(
                RemoteProposalError::Validation(ProposalValidationError::Unsubscribed(unsubs_id)),
                _,
            ),
        ) => {
            assert_eq!(proposal_id_send, proposal0_id);
            assert_eq!(unsubs_id, offer_id);
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
        ProposalError::Send(
            proposal_id_send,
            CounterProposalError::Remote(
                RemoteProposalError::Validation(ProposalValidationError::Unsubscribed(unsubs_id)),
                _,
            ),
        ) => {
            assert_eq!(proposal_id_send, proposal0_id);
            assert_eq!(unsubs_id, offer_id);
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

/// Requestor tries to reject initial Proposal
/// (Provider Node does not even know that there is a Proposal).
/// Negotiation attempt should be rejected by Provider Node.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_reject_initial_offer() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Req-1")
        .await
        .add_market_instance("Prov-1")
        .await;

    let req_mkt = network.get_market("Req-1");
    let prov_mkt = network.get_market("Prov-1");

    let req_id = network.get_default_id("Req-1");
    let prov_id = network.get_default_id("Prov-1");

    let demand_id = req_mkt
        .subscribe_demand(&sample_demand(), &req_id)
        .await
        .unwrap();
    let _offer_id = prov_mkt
        .subscribe_offer(&sample_offer(), &prov_id)
        .await
        .unwrap();

    let proposal0 = requestor::query_proposal(&req_mkt, &demand_id, "Initial #R")
        .await
        .unwrap();
    let proposal0id = &proposal0.get_proposal_id().unwrap();

    req_mkt
        .requestor_engine
        .reject_proposal(&demand_id, &proposal0id, &req_id, Some("dblah".into()))
        .await
        .map_err(|e| panic!("Expected Ok(()), got: {}\nDEBUG: {:?}", e.to_string(), e))
        .unwrap();

    req_mkt
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await
        .map_err(|e| panic!("Expected Ok([]), got: {}\nDEBUG: {:?}", e.to_string(), e))
        .map(|events| assert_eq!(events.len(), 0))
        .unwrap();

    let proposal0updated = req_mkt.get_proposal(&proposal0id).await.unwrap();

    assert_eq!(proposal0updated.body.state, ProposalState::Rejected);
}

/// Provider rejects draft Proposal and succeeds.
/// As a result Proposal is in Rejected state on both sides.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_reject_demand() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Req-1")
        .await
        .add_market_instance("Prov-1")
        .await;

    let req_mkt = network.get_market("Req-1");
    let prov_mkt = network.get_market("Prov-1");

    let req_id = network.get_default_id("Req-1");
    let prov_id = network.get_default_id("Prov-1");

    let demand = sample_demand();
    let demand_id = req_mkt.subscribe_demand(&demand, &req_id).await.unwrap();
    let offer_id = prov_mkt
        .subscribe_offer(&sample_offer(), &prov_id)
        .await
        .unwrap();

    let proposal0 = requestor::query_proposal(&req_mkt, &demand_id, "Initial #R")
        .await
        .unwrap();
    let proposal0id = &proposal0.get_proposal_id().unwrap();

    let req_demand_proposal1_id = req_mkt
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0id, &demand, &req_id)
        .await
        .unwrap();

    // Provider receives Proposal
    let _prov_demand_proposal1 = provider::query_proposal(&prov_mkt, &offer_id, "Initial #P")
        .await
        .unwrap();
    let prov_demand_proposal1_id = req_demand_proposal1_id.clone().translate(Owner::Provider);

    // Provider rejects Proposal with reason.
    prov_mkt
        .provider_engine
        .reject_proposal(
            &offer_id,
            &prov_demand_proposal1_id,
            &prov_id,
            Some("zima".into()),
        )
        .await
        .unwrap();

    // Requestor receives Rejection with reason
    req_mkt
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await
        .map_err(|e| panic!("Expected Ok([ev]), got: {}\nDEBUG: {:?}", e.to_string(), e))
        .map(|events| {
            assert_eq!(events.len(), 1);
            match &events[0] {
                RequestorEvent::ProposalRejectedEvent { reason, .. } => {
                    assert_eq!(reason, &Some("zima".into()))
                }
                event => panic!("Expected ProposalRejectedEvent, got: {:?}", event),
            }
        })
        .unwrap();

    let proposal0updated = prov_mkt
        .get_proposal(&prov_demand_proposal1_id)
        .await
        .unwrap();
    assert_eq!(proposal0updated.body.state, ProposalState::Rejected);
}

// Events with proposals should come last
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_proposal_events_last() {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await
        .add_market_instance("Node-2")
        .await
        .add_market_instance("Node-3")
        .await;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");
    let market3 = network.get_market("Node-3");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");
    let identity3 = network.get_default_id("Node-3");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await
        .unwrap();

    let offer1_id = market2
        .subscribe_offer(&sample_offer(), &identity2)
        .await
        .unwrap();

    // REQUESTOR side.
    let proposal0 = requestor::query_proposal(&market1, &demand_id, "Initial #R")
        .await
        .unwrap();
    let proposal0_id = proposal0.get_proposal_id().unwrap();

    // Counter proposal
    let proposal1 = sample_demand();
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await
        .unwrap();

    let offer2_id = market3
        .subscribe_offer(&sample_offer(), &identity3)
        .await
        .unwrap();

    // wait for Offer broadcast.
    assert_offers_broadcasted(&[&market1], &[offer2_id]).await;

    let proposal2 = provider::query_proposal(&market2, &offer1_id, "Initial #P")
        .await
        .unwrap();
    let proposal2_id = proposal2.get_proposal_id().unwrap();
    market2
        .provider_engine
        .reject_proposal(&offer1_id, &proposal2_id, &identity2, None)
        .await
        .unwrap();

    let events = market1
        .requestor_engine
        .query_events(&demand_id, 3.0, Some(5))
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    match events[0] {
        RequestorEvent::ProposalRejectedEvent { .. } => {}
        _ => assert!(false, "Invalid first event_type: {:#?}", events[0]),
    }
    match events[events.len() - 1] {
        RequestorEvent::ProposalEvent { .. } => {}
        _ => assert!(false, "Invalid last event_type: {:#?}", events[0]),
    }
}
