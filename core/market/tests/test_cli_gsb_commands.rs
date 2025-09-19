use ya_framework_basic::log::enable_logs;
use ya_framework_mocks::market::legacy::mock_node::MarketsNetwork;
use ya_framework_mocks::net::MockNet;

use ya_agreement_utils::agreement::flatten_value;
use ya_core_model::market::{local as market_bus, GetGolemBaseOffer};
use ya_framework_basic::temp_dir;
use ya_market::testing::{mock_offer::client::sample_offer, MarketServiceExt};
use ya_service_bus::RpcEndpoint;

const PROV_NAME: &str = "Node-1";

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_gsb_market_golembase_get_offer() -> anyhow::Result<()> {
    enable_logs(false);
    let dir = temp_dir!("test_gsb_market_golembase_get_offer")?;
    let dir = dir.path();

    let network = MarketsNetwork::new_containerized(dir, MockNet::new())
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let market = network.get_market(PROV_NAME);
    let identity = network.get_default_id(PROV_NAME).await;

    // Create an offer
    let offer_id = market
        .subscribe_offer(&sample_offer(), &identity)
        .await
        .unwrap();

    let offer = market.get_offer(&offer_id).await.unwrap();
    log::info!("Created offer with ID: {}", offer_id);

    // Test GSB command to get the offer
    let request = GetGolemBaseOffer {
        offer_id: offer_id.to_string(),
    };

    let gsb = market_bus::build_discovery_bindpoint(&network.market_gsb_prefixes(PROV_NAME));
    let response = gsb.local().send(request).await.unwrap().unwrap();

    // Verify the offer was retrieved correctly
    assert_eq!(response.offer.offer_id, offer_id.to_string());

    // Deserialize and compare properties
    let original_properties = flatten_value(serde_json::from_str(&offer.properties).unwrap());
    let response_properties = flatten_value(response.offer.properties);

    assert_eq!(response_properties, original_properties);
    assert_eq!(response.offer.constraints, offer.constraints);
    assert_eq!(response.offer.provider_id, offer.node_id);

    // Test error handling for non-existing offer
    let non_existing_id = "0000000000000000000000000000000000000000000000000000000000000001";
    let request = GetGolemBaseOffer {
        offer_id: non_existing_id.to_string(),
    };

    let result = gsb.local().send(request).await.unwrap();

    // Check that the error contains appropriate string
    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();

    // The error should contain information about the non-existing offer
    assert!(
        error_string.contains("not found"),
        "Error message should contain appropriate string about non-existing offer, got: {}",
        error_string
    );

    Ok(())
}
