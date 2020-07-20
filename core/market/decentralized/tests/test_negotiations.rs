use ya_market_decentralized::testing::events_helper::{provider, requestor, ClientProposalHelper};
use ya_market_decentralized::testing::mock_offer::client::{sample_demand, sample_offer};
use ya_market_decentralized::testing::MarketsNetwork;
use ya_market_decentralized::testing::OwnerType;
use ya_market_decentralized::testing::ProposalError;

use ya_client::model::market::proposal::State;

/// Test countering initial and draft proposals on both Provider and Requestor side.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_exchanging_draft_proposals() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_exchanging_draft_proposals")
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

    // Expect events generated on requestor market.
    let events = market1
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let proposal0 = requestor::expect_proposal(events)?;

    // Requestor counters initial proposal. We expect that provider will get proposal event.
    let proposal1_req = proposal0.counter_demand(sample_demand())?;
    let proposal1_req_id = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal1_req)
        .await?;

    // Provider receives Proposal
    let events = market2
        .provider_engine
        .query_events(&offer_id, 1.2, Some(5))
        .await?;
    let proposal1_prov = provider::expect_proposal(events)?;
    let proposal1_prov_id = proposal1_req_id.clone().translate(OwnerType::Provider);

    assert_eq!(proposal1_req.constraints, proposal1_prov.constraints);
    assert_eq!(proposal1_req.properties, proposal1_prov.properties);
    assert_eq!(proposal1_prov.state, Some(State::Draft));
    assert_eq!(
        Some(identity1.identity.to_string()),
        proposal1_prov.issuer_id
    );
    assert_eq!(proposal1_prov_id, proposal1_prov.get_proposal_id()?);

    // Provider counters proposal.
    let proposal2_prov = proposal1_prov.counter_offer(sample_offer())?;
    let proposal2_id = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal1_prov_id, &proposal2_prov)
        .await?;

    // Requestor receives proposal.
    let events = market1
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let proposal2_req = requestor::expect_proposal(events)?;
    let proposal2_req_id = proposal2_id.clone().translate(OwnerType::Requestor);

    assert_eq!(proposal2_req.constraints, proposal2_prov.constraints);
    assert_eq!(proposal2_req.properties, proposal2_prov.properties);
    assert_eq!(proposal2_req.state, Some(State::Draft));
    assert_eq!(
        Some(identity2.identity.to_string()),
        proposal2_req.issuer_id
    );
    assert_eq!(proposal2_req_id, proposal2_req.get_proposal_id()?);

    // Requestor counters draft proposal.
    let proposal3_req = proposal2_req.counter_demand(sample_demand())?;
    let proposal3_req_id = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal2_req_id, &proposal3_req)
        .await?;

    // Provider receives Proposal
    let events = market2
        .provider_engine
        .query_events(&offer_id, 1.2, Some(5))
        .await?;
    let proposal3_prov = provider::expect_proposal(events)?;
    let proposal3_prov_id = proposal3_req_id.clone().translate(OwnerType::Provider);

    assert_eq!(proposal3_req.constraints, proposal3_prov.constraints);
    assert_eq!(proposal3_req.properties, proposal3_prov.properties);
    assert_eq!(proposal3_prov.state, Some(State::Draft));
    assert_eq!(
        Some(identity1.identity.to_string()),
        proposal3_prov.issuer_id
    );
    assert_eq!(proposal3_prov_id, proposal3_prov.get_proposal_id()?);

    Ok(())
}

/// Can't counter proposal, that was already countered.
/// Market should reject such attempts.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_counter_countered_proposal() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_counter_countered_proposal")
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
    let events = market1
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let proposal0 = requestor::expect_proposal(events)?;
    let proposal0_id = proposal0.get_proposal_id()?;

    // Counter proposal for the first time.
    let proposal1 = proposal0.counter_demand(sample_demand())?;
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal1)
        .await?;

    // Now counter proposal for the second time. It should fail.
    let proposal2 = proposal0.counter_demand(sample_demand())?;
    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal2)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::AlreadyCountered(id) => assert_eq!(id, proposal0_id),
        _ => panic!("Expected ProposalError::AlreadyCountered."),
    }

    // PROVIDER side
    let events = market2
        .provider_engine
        .query_events(&offer_id, 1.2, Some(5))
        .await?;
    let proposal0 = provider::expect_proposal(events)?;
    let proposal0_id = proposal0.get_proposal_id()?;

    // Counter proposal for the first time.
    let proposal1 = proposal0.counter_offer(sample_offer())?;
    market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal0.get_proposal_id()?, &proposal1)
        .await?;

    // Now counter proposal for the second time. It should fail.
    let proposal2 = proposal0.counter_offer(sample_offer())?;
    let result = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal0.get_proposal_id()?, &proposal2)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::AlreadyCountered(id) => assert_eq!(id, proposal0_id),
        _ => panic!("Expected ProposalError::AlreadyCountered."),
    }

    Ok(())
}

/// Can't counter own proposal.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_counter_own_proposal() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_counter_own_proposal")
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
    let events = market1
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let proposal0 = requestor::expect_proposal(events)?;

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    let proposal1_id = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal1)
        .await?;

    // Counter proposal1, that was created by us.
    let mut proposal2 = proposal0.counter_demand(sample_demand())?;
    proposal2.prev_proposal_id = Some(proposal1_id.to_string());

    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal1_id, &proposal2)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::OwnProposal(id) => assert_eq!(id, proposal1_id),
        _ => panic!("Expected ProposalError::OwnProposal."),
    }

    // PROVIDER side
    let events = market2
        .provider_engine
        .query_events(&offer_id, 1.2, Some(5))
        .await?;
    let proposal0 = provider::expect_proposal(events)?;

    let proposal1 = proposal0.counter_offer(sample_offer())?;
    let proposal1_id = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal0.get_proposal_id()?, &proposal1)
        .await?;

    // Counter proposal1, that was created by us.
    let mut proposal2 = proposal0.counter_offer(sample_offer())?;
    proposal2.prev_proposal_id = Some(proposal1_id.to_string());

    let result = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal1_id, &proposal2)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::OwnProposal(id) => assert_eq!(id, proposal1_id),
        _ => panic!("Expected ProposalError::OwnProposal."),
    }

    Ok(())
}

/// Can't counter Proposal, for which Demand was unsubscribed.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_counter_unsubscribed_demand() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_counter_unsubscribed")
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

    let events = market1
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let proposal0 = requestor::expect_proposal(events)?;
    market1.unsubscribe_demand(&demand_id, &identity1).await?;

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::NoSubscription(id) => assert_eq!(id, demand_id),
        _ => panic!("Expected ProposalError::Unsubscribed."),
    }

    Ok(())
}

/// Can't counter Proposal, for which Offer was unsubscribed.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_counter_unsubscribed_offer() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_counter_unsubscribed_offer")
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

    let events = market1
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let proposal0 = requestor::expect_proposal(events)?;

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal1)
        .await?;

    // PROVIDER side
    let events = market2
        .provider_engine
        .query_events(&offer_id, 1.2, Some(5))
        .await?;
    market2.unsubscribe_offer(&offer_id, &identity2).await?;

    let proposal0 = provider::expect_proposal(events)?;
    let proposal1 = proposal0.counter_offer(sample_offer())?;
    let result = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal0.get_proposal_id()?, &proposal1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::Unsubscribed(id) => assert_eq!(id, offer_id),
        _ => panic!("Expected ProposalError::Unsubscribed."),
    }

    Ok(())
}
