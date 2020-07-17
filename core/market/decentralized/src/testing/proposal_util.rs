use std::str::FromStr;

use crate::db::model::ProposalId;
use crate::testing::events_helper::ClientProposalHelper;
use crate::testing::events_helper::{provider, requestor};
use crate::testing::mock_offer::client::{sample_demand, sample_offer};
use crate::testing::MarketsNetwork;
use crate::testing::OwnerType;

pub async fn exchange_draft_proposals(
    network: &MarketsNetwork,
    node_id1: &str,
    node_id2: &str,
) -> Result<ProposalId, anyhow::Error> {
    let market1 = network.get_market(node_id1);
    let market2 = network.get_market(node_id2);

    let identity1 = network.get_default_id(node_id1);
    let identity2 = network.get_default_id(node_id2);

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

    // Provider counters proposal.
    let proposal2_prov = proposal1_prov.counter_offer(sample_offer())?;
    let _proposal2_id = market2
        .provider_engine
        .counter_proposal(&offer_id, &proposal1_prov_id, &proposal2_prov)
        .await?;

    // Requestor receives proposal.
    let events = market1
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let proposal2_req = requestor::expect_proposal(events)?;
    Ok(ProposalId::from_str(&proposal2_req.proposal_id.unwrap())?)
}
