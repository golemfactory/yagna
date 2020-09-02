use chrono::{Duration, NaiveDateTime, Utc};
use std::str::FromStr;

use crate::db::model::{
    DbProposal, IssuerType, Negotiation, ProposalId, ProposalState, SubscriptionId,
};
use crate::testing::events_helper::ClientProposalHelper;
use crate::testing::events_helper::{provider, requestor};
use crate::testing::mock_offer::client::{sample_demand, sample_offer};
use crate::testing::MarketsNetwork;
use crate::testing::OwnerType;
use ya_client::model::NodeId;

pub fn generate_proposal(
    unifier: i64,
    expiration_ts: NaiveDateTime,
    negotiation_id: String,
) -> DbProposal {
    DbProposal {
        id: ProposalId::generate_id(
            &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
            &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
            // Add parametrized integer - unifier to ensure unique ids
            &(Utc::now() + Duration::days(unifier)).naive_utc(),
            OwnerType::Requestor,
        ),
        prev_proposal_id: None,
        issuer: IssuerType::Them,
        negotiation_id,
        properties: "".to_string(),
        constraints: "".to_string(),
        state: ProposalState::Initial,
        creation_ts: Utc::now().naive_utc(),
        expiration_ts,
    }
}

pub fn generate_negotiation(agreement_id: Option<ProposalId>) -> Negotiation {
    use uuid::Uuid;
    Negotiation {
        id: format!("{}", Uuid::new_v4()),
        subscription_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        offer_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        demand_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        provider_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        requestor_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        agreement_id,
    }
}

pub async fn exchange_draft_proposals(
    network: &MarketsNetwork,
    req_name: &str,
    prov_name: &str,
) -> Result<ProposalId, anyhow::Error> {
    let req_mkt = network.get_market(req_name);
    let prov_mkt = network.get_market(prov_name);

    let req_id = network.get_default_id(req_name);
    let prov_id = network.get_default_id(prov_name);

    let demand_id = req_mkt.subscribe_demand(&sample_demand(), &req_id).await?;
    let offer_id = prov_mkt.subscribe_offer(&sample_offer(), &prov_id).await?;

    // Expect events generated on requestor market.
    let req_events = req_mkt
        .requestor_engine
        .query_events(&demand_id, 3.14, Some(5))
        .await?;
    let req_offer_proposal1 = requestor::expect_proposal(req_events, 1)?;

    // Requestor counters initial proposal. We expect that provider will get proposal event.
    let req_demand_proposal1 = req_offer_proposal1.counter_demand(sample_demand())?;
    let req_demand_proposal1_id = req_mkt
        .requestor_engine
        .counter_proposal(
            &demand_id,
            &req_offer_proposal1.get_proposal_id()?,
            &req_demand_proposal1,
        )
        .await?;

    // Provider receives Proposal
    let prov_events = prov_mkt
        .provider_engine
        .query_events(&offer_id, 3.14, Some(5))
        .await?;
    let prov_demand_proposal1 = provider::expect_proposal(prov_events, 2)?;
    let prov_demand_proposal1_id = req_demand_proposal1_id
        .clone()
        .translate(OwnerType::Provider);

    // Provider counters proposal.
    let offer_proposal2 = prov_demand_proposal1.counter_offer(sample_offer())?;
    let _offer_proposal_id = prov_mkt
        .provider_engine
        .counter_proposal(&offer_id, &prov_demand_proposal1_id, &offer_proposal2)
        .await?;

    // Requestor receives proposal.
    let req_events = req_mkt
        .requestor_engine
        .query_events(&demand_id, 1.2, Some(5))
        .await?;
    let req_offer_proposal2 = requestor::expect_proposal(req_events, 3)?;
    Ok(ProposalId::from_str(
        &req_offer_proposal2.proposal_id.unwrap(),
    )?)
}
