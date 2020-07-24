use ya_market_decentralized::testing::events_helper::{provider, requestor, ClientProposalHelper};
use ya_market_decentralized::testing::mock_offer::client::{sample_demand, sample_offer};
use ya_market_decentralized::testing::MarketsNetwork;
use ya_market_decentralized::testing::OwnerType;

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
    let offer = sample_offer();
    let offer_id = market2.subscribe_offer(&offer, &identity2).await?;

    // Expect events generated on requestor market.
    let events = market1
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let proposal0 = requestor::expect_proposal(events, 1)?;
    assert_eq!(proposal0.properties, offer.properties);
    assert_eq!(proposal0.constraints, offer.constraints);
    assert!(proposal0.proposal_id.is_some());
    assert_eq!(proposal0.issuer_id, Some(identity2.identity.to_string()));
    assert_eq!(proposal0.state, Some(State::Initial));
    assert_eq!(proposal0.prev_proposal_id, None);

    // Requestor counters initial proposal. We expect that provider will get proposal event.
    let proposal1_req = proposal0.counter_demand(sample_demand())?;
    let proposal1_req_id = market1
        .requestor_engine
        .counter_proposal(&demand_id, &proposal0.get_proposal_id()?, &proposal1_req)
        .await?;
    assert_eq!(proposal1_req.prev_proposal_id, proposal0.proposal_id);

    // Provider receives Proposal
    let events = market2
        .provider_engine
        .query_events(&offer_id, 1.2, Some(5))
        .await?;
    let proposal1_prov = provider::expect_proposal(events, 2)?;
    let proposal1_prov_id = proposal1_req_id.clone().translate(OwnerType::Provider);

    assert_eq!(proposal1_prov.constraints, proposal1_req.constraints);
    assert_eq!(proposal1_prov.properties, proposal1_req.properties);
    assert_eq!(
        proposal1_prov.proposal_id,
        Some(proposal1_prov_id.to_string()),
    );
    assert_eq!(
        proposal1_prov.issuer_id,
        Some(identity1.identity.to_string()),
    );
    assert_eq!(proposal1_prov.state, Some(State::Draft));
    assert_eq!(proposal1_prov.prev_proposal_id, None);

    // Provider counters proposal.
    let proposal2_prov = proposal1_prov.counter_offer(sample_offer())?;
    let proposal2_id = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal1_prov_id, &proposal2_prov)
        .await?;
    assert_eq!(proposal2_prov.prev_proposal_id, proposal1_prov.proposal_id);

    // Requestor receives proposal.
    let events = market1
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let proposal2_req = requestor::expect_proposal(events, 3)?;
    let proposal2_req_id = proposal2_id.clone().translate(OwnerType::Requestor);

    assert_eq!(proposal2_req.constraints, proposal2_prov.constraints);
    assert_eq!(proposal2_req.properties, proposal2_prov.properties);
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
        .counter_proposal(&demand_id, &proposal2_req_id, &proposal3_req)
        .await?;
    assert_eq!(proposal3_req.prev_proposal_id, proposal2_req.proposal_id);

    // Provider receives Proposal
    let events = market2
        .provider_engine
        .query_events(&offer_id, 1.2, Some(5))
        .await?;
    let proposal3_prov = provider::expect_proposal(events, 4)?;
    let proposal3_prov_id = proposal3_req_id.clone().translate(OwnerType::Provider);

    assert_eq!(proposal3_prov.constraints, proposal3_req.constraints);
    assert_eq!(proposal3_prov.properties, proposal3_req.properties);
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
