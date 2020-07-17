use chrono::Utc;

use ya_core_model::market;
use ya_market_decentralized::testing::proposal_util::exchange_draft_proposals;
use ya_market_decentralized::testing::MarketsNetwork;
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_gsb_get_agreement() -> Result<(), anyhow::Error> {
    let node_id1 = "Node-1";
    let node_id2 = "Node-2";
    let network = MarketsNetwork::new("test_gsb_get_agreement")
        .await
        .add_market_instance(node_id1)
        .await?
        .add_market_instance(node_id2)
        .await?;

    let proposal_id = exchange_draft_proposals(&network, node_id1, node_id2).await?;
    let market = network.get_market(node_id1);
    let identity1 = network.get_default_id(node_id1);
    let identity2 = network.get_default_id(node_id2);

    let agreement_id = market
        .requestor_engine
        .create_agreement(identity1.clone(), &proposal_id, Utc::now())
        .await?;
    let agreement = bus::service(network.node_gsb_prefixes(node_id1).0)
        .send(market::GetAgreement {
            agreement_id: agreement_id.to_string(),
        })
        .await??;
    assert_eq!(agreement.agreement_id, agreement_id.to_string());
    assert_eq!(
        agreement.demand.requestor_id.unwrap(),
        identity1.identity.to_string()
    );
    assert_eq!(
        agreement.offer.provider_id.unwrap(),
        identity2.identity.to_string()
    );
    Ok(())
}
