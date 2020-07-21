use ya_market_decentralized::testing::events_helper::{provider, requestor, ClientProposalHelper};
use ya_market_decentralized::testing::mock_offer::client::{sample_demand, sample_offer};
use ya_market_decentralized::testing::proposal_util::{
    exchange_draft_proposals, NegotiationHelper,
};
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
    let proposal0 = requestor::query_proposal(&market1, &demand_id).await?;

    // Requestor counters initial proposal. We expect that provider will get proposal event.
    let proposal1_req = proposal0.counter_demand(sample_demand())?;
    let proposal1_req_id = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal1_req)
        .await?;

    // Provider receives Proposal
    let proposal1_prov = provider::query_proposal(&market2, &offer_id).await?;
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
    let proposal2_req = requestor::query_proposal(&market1, &demand_id).await?;
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
    let proposal3_prov = provider::query_proposal(&market2, &offer_id).await?;
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
    let proposal0 = requestor::query_proposal(&market1, &demand_id).await?;
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
    let proposal0 = provider::query_proposal(&market2, &offer_id).await?;
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
    let proposal0 = requestor::query_proposal(&market1, &demand_id).await?;

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
    let proposal0 = provider::query_proposal(&market2, &offer_id).await?;

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

    let proposal0 = requestor::query_proposal(&market1, &demand_id).await?;
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

    let proposal0 = requestor::query_proposal(&market1, &demand_id).await?;
    let proposal1 = proposal0.counter_demand(sample_demand())?;
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal1)
        .await?;

    // PROVIDER side
    let proposal0 = provider::query_proposal(&market2, &offer_id).await?;
    market2.unsubscribe_offer(&offer_id, &identity2).await?;

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

/// Requestor tries to counter initial Proposal, for which Offer was unsubscribed on remote Node.
/// Negotiation attempt should be rejected by Provider Node.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_counter_initial_unsubscribed_remote_offer() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_counter_initial_unsubscribed_remote_offer")
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

    let proposal0 = requestor::query_proposal(&market1, &demand_id).await?;

    // When we will counter this Proposal, Provider will have it already unsubscribed.
    market2.unsubscribe_offer(&offer_id, &identity2).await?;

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::FailedSendProposal(..) => (),
        _ => panic!("Expected ProposalError::FailedSendProposal."),
    }

    Ok(())
}

/// Requestor tries to counter draft Proposal, for which Offer was unsubscribed on remote Node.
/// Negotiation attempt should be rejected by Provider Node.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_counter_draft_unsubscribed_remote_offer() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_counter_initial_unsubscribed_remote_offer")
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
    let identity2 = network.get_default_id("Node-2");

    // When we will counter this Proposal, Provider will have it already unsubscribed.
    market2.unsubscribe_offer(&offer_id, &identity2).await?;

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    let result = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::FailedSendProposal(..) => (),
        _ => panic!("Expected ProposalError::FailedSendProposal."),
    }

    Ok(())
}

/// Provider tries to counter draft Proposal, for which Demand was unsubscribed on remote Node.
/// Negotiation attempt should be rejected by Requestor Node.
#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_counter_draft_unsubscribed_remote_demand() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new("test_counter_draft_unsubscribed_remote_demand")
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

    let proposal1 = proposal0.counter_demand(sample_demand())?;
    market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0_id, &proposal1)
        .await?;

    let proposal2 = provider::query_proposal(&market2, &offer_id).await?;
    market1.unsubscribe_demand(&demand_id, &identity1).await?;

    let proposal3 = proposal2.counter_offer(sample_offer())?;
    let result = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal2.get_proposal_id()?, &proposal3)
        .await;

    assert!(result.is_err());
    match result.err().unwrap() {
        ProposalError::FailedSendProposal(..) => (),
        _ => panic!("Expected ProposalError::FailedSendProposal."),
    }
    Ok(())
}
