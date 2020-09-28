use ya_client::model::market::proposal::State;
use ya_market_decentralized::testing::events_helper::{provider, requestor, ClientProposalHelper};
use ya_market_decentralized::testing::mock_offer::client::{
    not_matching_demand, not_matching_offer, sample_demand, sample_offer,
};
use ya_market_decentralized::testing::proposal_util::{
    exchange_draft_proposals, NegotiationHelper,
};
use ya_market_decentralized::testing::MarketsNetwork;
use ya_market_decentralized::testing::OwnerType;
use ya_market_decentralized::testing::{ProposalError, SaveProposalError};
use ya_market_resolver::flatten::flatten_json;

/// Test countering initial and draft proposals on both Provider and Requestor side.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_exchanging_draft_proposals() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await?;
    let offer = sample_offer();
    let offer_id = market2.subscribe_offer(&offer, &identity2).await?;

    // Expect events generated on requestor market.
    let proposal0 = requestor::query_proposal(&market1, &demand_id, 1).await?;
    assert_eq!(
        proposal0.properties,
        flatten_json(&offer.properties).unwrap()
    );
    assert_eq!(proposal0.constraints, offer.constraints);
    assert!(proposal0.proposal_id.is_some());
    assert_eq!(proposal0.issuer_id, Some(identity2.identity.to_string()));
    assert_eq!(proposal0.state, Some(State::Initial));
    assert_eq!(proposal0.prev_proposal_id, None);

    // Requestor counters initial proposal. We expect that provider will get proposal event.
    let proposal1_req = proposal0.counter_demand(sample_demand())?;
    let proposal1_req_id = market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id()?,
            &proposal1_req,
            &identity1,
        )
        .await?;
    assert_eq!(proposal1_req.prev_proposal_id, proposal0.proposal_id);

    // Provider receives Proposal
    let proposal1_prov = provider::query_proposal(&market2, &offer_id, 2).await?;
    let proposal1_prov_id = proposal1_req_id.clone().translate(OwnerType::Provider);

    assert_eq!(proposal1_prov.constraints, proposal1_req.constraints);
    assert_eq!(
        proposal1_prov.properties,
        flatten_json(&proposal1_req.properties).unwrap()
    );
    assert_eq!(
        proposal1_prov.proposal_id,
        Some(proposal1_prov_id.to_string()),
    );
    assert_eq!(
        proposal1_prov.issuer_id,
        Some(identity1.identity.to_string()),
    );
    assert_eq!(proposal1_prov.state, Some(State::Draft));
    // Requestor and Provider have different first Proposals IDs
    assert!(proposal1_prov.prev_proposal_id.is_some());

    // Provider counters proposal.
    let proposal2_prov = proposal1_prov.counter_offer(sample_offer())?;
    let proposal2_id = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal1_prov_id, &proposal2_prov, &identity2)
        .await?;
    assert_eq!(proposal2_prov.prev_proposal_id, proposal1_prov.proposal_id);

    // Requestor receives proposal.
    let proposal2_req = requestor::query_proposal(&market1, &demand_id, 3).await?;
    let proposal2_req_id = proposal2_id.clone().translate(OwnerType::Requestor);

    assert_eq!(proposal2_req.constraints, proposal2_prov.constraints);
    assert_eq!(
        proposal2_req.properties,
        flatten_json(&proposal2_prov.properties).unwrap()
    );
    assert_eq!(
        proposal2_req.proposal_id,
        Some(proposal2_req_id.to_string()),
    );
    assert_eq!(
        proposal2_req.issuer_id,
        Some(identity2.identity.to_string()),
    );
    assert_eq!(proposal2_req.state, Some(State::Draft));
    assert_eq!(
        proposal2_req.prev_proposal_id,
        Some(
            proposal1_prov_id
                .translate(OwnerType::Requestor)
                .to_string()
        ),
    );

    // Requestor counters draft proposal.
    let proposal3_req = proposal2_req.counter_demand(sample_demand())?;
    let proposal3_req_id = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal2_req_id, &proposal3_req, &identity1)
        .await?;
    assert_eq!(proposal3_req.prev_proposal_id, proposal2_req.proposal_id);

    // Provider receives Proposal
    let proposal3_prov = provider::query_proposal(&market2, &offer_id, 4).await?;
    let proposal3_prov_id = proposal3_req_id.clone().translate(OwnerType::Provider);

    assert_eq!(proposal3_prov.constraints, proposal3_req.constraints);
    assert_eq!(
        proposal3_prov.properties,
        flatten_json(&proposal3_req.properties).unwrap()
    );
    assert_eq!(
        proposal3_prov.proposal_id,
        Some(proposal3_prov_id.to_string()),
    );
    assert_eq!(
        proposal3_prov.issuer_id,
        Some(identity1.identity.to_string()),
    );
    assert_eq!(proposal3_prov.state, Some(State::Draft));
    assert_eq!(
        proposal3_prov.prev_proposal_id,
        Some(proposal2_req_id.translate(OwnerType::Provider).to_string()),
    );

    Ok(())
}

/// Can't counter proposal, that was already countered.
/// Market should reject such attempts.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_counter_countered_proposal() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await?;
    let offer_id = market2.subscribe_offer(&sample_offer(), &identity2).await?;

    // REQUESTOR side.
    let proposal0 = requestor::query_proposal(&market1, &demand_id, 1).await?;
    let proposal0_id = proposal0.get_proposal_id()?;

    // Counter proposal for the first time.
    let proposal1 = proposal0.counter_demand(sample_demand())?;
    market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id()?,
            &proposal1,
            &identity1,
        )
        .await?;

    // Now counter proposal for the second time. It should fail.
    let proposal2 = proposal0.counter_demand(sample_demand())?;
    let result = market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id()?,
            &proposal2,
            &identity1,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Save(SaveProposalError::AlreadyCountered(id)) => {
            assert_eq!(id, proposal0_id);
        }
        _ => panic!("Expected AlreadyCountered error."),
    }

    // PROVIDER side
    let proposal0 = provider::query_proposal(&market2, &offer_id, 2).await?;
    let proposal0_id = proposal0.get_proposal_id()?;

    // Counter proposal for the first time.
    let proposal1 = proposal0.counter_offer(sample_offer())?;
    market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal0.get_proposal_id()?,
            &proposal1,
            &identity2,
        )
        .await?;

    // Now counter proposal for the second time. It should fail.
    let proposal2 = proposal0.counter_offer(sample_offer())?;
    let result = market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal0.get_proposal_id()?,
            &proposal2,
            &identity2,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Save(SaveProposalError::AlreadyCountered(id)) => {
            assert_eq!(id, proposal0_id)
        }
        _ => panic!("Expected AlreadyCountered error."),
    }

    Ok(())
}

/// Can't counter own proposal.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_counter_own_proposal() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await?;
    let offer_id = market2.subscribe_offer(&sample_offer(), &identity2).await?;

    // REQUESTOR side.
    let proposal0 = requestor::query_proposal(&market1, &demand_id, 1).await?;

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    let proposal1_id = market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id()?,
            &proposal1,
            &identity1,
        )
        .await?;

    // Counter proposal1, that was created by us.
    let mut proposal2 = proposal0.counter_demand(sample_demand())?;
    proposal2.prev_proposal_id = Some(proposal1_id.to_string());

    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal1_id, &proposal2, &identity1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::OwnProposal(id) => assert_eq!(id, proposal1_id),
        _ => panic!("Expected ProposalError::OwnProposal."),
    }

    // PROVIDER side
    let proposal0 = provider::query_proposal(&market2, &offer_id, 2).await?;

    let proposal1 = proposal0.counter_offer(sample_offer())?;
    let proposal1_id = market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal0.get_proposal_id()?,
            &proposal1,
            &identity2,
        )
        .await?;

    // Counter proposal1, that was created by us.
    let mut proposal2 = proposal0.counter_offer(sample_offer())?;
    proposal2.prev_proposal_id = Some(proposal1_id.to_string());

    let result = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal1_id, &proposal2, &identity2)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::OwnProposal(id) => assert_eq!(id, proposal1_id),
        _ => panic!("Expected ProposalError::OwnProposal."),
    }

    Ok(())
}

/// Requestor can't counter Proposal, for which he has unsubscribed Demand.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_counter_unsubscribed_demand() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await?;
    market2.subscribe_offer(&sample_offer(), &identity2).await?;

    let proposal0 = requestor::query_proposal(&market1, &demand_id, 1).await?;
    market1.unsubscribe_demand(&demand_id, &identity1).await?;

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    let result = market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id()?,
            &proposal1,
            &identity1,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::NoSubscription(id) => assert_eq!(id, demand_id),
        _ => panic!("Expected ProposalError::Unsubscribed."),
    }

    Ok(())
}

/// Provider can't counter Proposal, for which he has unsubscribed Offer.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_counter_unsubscribed_offer() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await?;
    let offer_id = market2.subscribe_offer(&sample_offer(), &identity2).await?;

    let proposal0 = requestor::query_proposal(&market1, &demand_id, 1).await?;
    let proposal1 = proposal0.counter_demand(sample_demand())?;
    market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id()?,
            &proposal1,
            &identity1,
        )
        .await?;

    // PROVIDER side
    let proposal0 = provider::query_proposal(&market2, &offer_id, 2).await?;
    market2.unsubscribe_offer(&offer_id, &identity2).await?;

    let proposal1 = proposal0.counter_offer(sample_offer())?;
    let result = market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal0.get_proposal_id()?,
            &proposal1,
            &identity2,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Unsubscribed(id) => assert_eq!(id, offer_id),
        _ => panic!("Expected ProposalError::Unsubscribed."),
    }

    Ok(())
}

/// Requestor tries to counter initial Proposal, for which Offer was unsubscribed on remote Node.
/// Negotiation attempt should be rejected by Provider Node.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_counter_initial_unsubscribed_remote_offer() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");

    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let demand_id = market1
        .subscribe_demand(&sample_demand(), &identity1)
        .await?;
    let offer_id = market2.subscribe_offer(&sample_offer(), &identity2).await?;

    let proposal0 = requestor::query_proposal(&market1, &demand_id, 1).await?;

    // When we will counter this Proposal, Provider will have it already unsubscribed.
    market2.unsubscribe_offer(&offer_id, &identity2).await?;

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    let result = market1
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &proposal0.get_proposal_id()?,
            &proposal1,
            &identity1,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Send(..) => (),
        _ => panic!("Expected ProposalError::Send."),
    }

    Ok(())
}

/// Requestor tries to counter draft Proposal, for which Offer was unsubscribed on remote Node.
/// Negotiation attempt should be rejected by Provider Node.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_counter_draft_unsubscribed_remote_offer() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let NegotiationHelper {
        proposal_id: proposal0_id,
        proposal: proposal0,
        offer_id,
        demand_id,
    } = exchange_draft_proposals(&network, "Node-1", "Node-2").await?;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");
    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    // When we will counter this Proposal, Provider will have it already unsubscribed.
    market2.unsubscribe_offer(&offer_id, &identity2).await?;

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Send(..) => (),
        _ => panic!("Expected ProposalError::Send."),
    }

    Ok(())
}

/// Provider tries to counter draft Proposal, for which Demand was unsubscribed on remote Node.
/// Negotiation attempt should be rejected by Requestor Node.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_counter_draft_unsubscribed_remote_demand() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let NegotiationHelper {
        proposal_id: proposal0_id,
        proposal: proposal0,
        offer_id,
        demand_id,
    } = exchange_draft_proposals(&network, "Node-1", "Node-2").await?;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");
    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await?;

    let proposal2 = provider::query_proposal(&market2, &offer_id, 1).await?;
    market1.unsubscribe_demand(&demand_id, &identity1).await?;

    let proposal3 = proposal2.counter_offer(sample_offer())?;
    let result = market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal2.get_proposal_id()?,
            &proposal3,
            &identity2,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Send(..) => (),
        _ => panic!("Expected ProposalError::Send."),
    }
    Ok(())
}

/// Try to send not matching counter Proposal to Provider. Our market
/// should reject such Proposal. Error should occur on Requestor side.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_not_matching_counter_demand() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let NegotiationHelper {
        proposal_id: proposal0_id,
        proposal: proposal0,
        demand_id,
        ..
    } = exchange_draft_proposals(&network, "Node-1", "Node-2").await?;

    let market1 = network.get_market("Node-1");
    let identity1 = network.get_default_id("Node-1");
    let proposal1 = proposal0.counter_demand(not_matching_demand())?;
    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::NotMatching(..) => (),
        _ => panic!("Expected ProposalError::NotMatching."),
    }

    Ok(())
}

/// Try to send not matching counter Proposal to Requestor. Our market
/// should reject such Proposal. Error should occur on Provider side.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_not_matching_counter_offer() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Node-1")
        .await?
        .add_market_instance("Node-2")
        .await?;

    let NegotiationHelper {
        proposal_id: proposal0_id,
        proposal: proposal0,
        demand_id,
        offer_id,
    } = exchange_draft_proposals(&network, "Node-1", "Node-2").await?;

    let market1 = network.get_market("Node-1");
    let market2 = network.get_market("Node-2");
    let identity1 = network.get_default_id("Node-1");
    let identity2 = network.get_default_id("Node-2");

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1, &identity1)
        .await?;

    let proposal2 = provider::query_proposal(&market2, &offer_id, 1).await?;
    let proposal3 = proposal2.counter_offer(not_matching_offer())?;
    let result = market2
        .provider_engine
        .counter_proposal(
            &offer_id,
            &proposal2.get_proposal_id()?,
            &proposal3,
            &identity2,
        )
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::NotMatching(..) => (),
        _ => panic!("Expected ProposalError::NotMatching."),
    }

    Ok(())
}
