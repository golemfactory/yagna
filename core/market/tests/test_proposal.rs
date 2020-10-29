use ya_market::assert_err_eq;
use ya_market::testing::proposal_util::exchange_draft_proposals;
use ya_market::testing::{
    GetProposalError, MarketServiceExt, MarketsNetwork, OwnerType, ProposalError,
};

use ya_client::model::market::proposal::State;

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_get_proposal() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Requestor1")
        .await?
        .add_market_instance("Provider1")
        .await?;

    let req_market = network.get_market("Requestor1");
    let prov_market = network.get_market("Provider1");
    let prov_id = network.get_default_id("Provider1");

    // Requestor side
    let proposal_id = exchange_draft_proposals(&network, "Requestor1", "Provider1")
        .await?
        .proposal_id;
    let result = req_market.get_proposal(&proposal_id).await;

    assert!(result.is_ok());
    let proposal = result.unwrap().into_client()?;

    assert_eq!(proposal.state()?, &State::Draft);
    assert_eq!(proposal.proposal_id()?, &proposal_id.to_string());
    assert_eq!(proposal.issuer_id()?, &prov_id.identity.to_string());
    assert!(proposal.prev_proposal_id().is_ok());

    // Provider side
    let proposal_id = proposal_id.translate(OwnerType::Provider);
    let result = prov_market.get_proposal(&proposal_id).await;

    assert!(result.is_ok());
    let proposal = result.unwrap().into_client()?;

    assert_eq!(proposal.state()?, &State::Draft);
    assert_eq!(proposal.proposal_id()?, &proposal_id.to_string());
    assert_eq!(proposal.issuer_id()?, &prov_id.identity.to_string());
    assert!(proposal.prev_proposal_id().is_ok());
    Ok(())
}

/// Try to query not existing Proposal.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[actix_rt::test]
#[serial_test::serial]
async fn test_get_proposal_not_found() -> Result<(), anyhow::Error> {
    let network = MarketsNetwork::new(None)
        .await
        .add_market_instance("Requestor1")
        .await?
        .add_market_instance("Provider1")
        .await?;

    let req_market = network.get_market("Requestor1");

    // Create some Proposals, that will be unused.
    exchange_draft_proposals(&network, "Requestor1", "Provider1").await?;

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
    Ok(())
}
