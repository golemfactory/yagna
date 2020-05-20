mod utils;

#[cfg(test)]
mod tests {
    use crate::utils::MarketsNetwork;

    use ya_client::model::market::Offer;
    use ya_market_decentralized::MarketService;

    use serde_json::json;
    use std::sync::Arc;

    #[cfg_attr(not(feature = "market-test-suite"), ignore)]
    #[actix_rt::test]
    async fn test_subscribe_offer() -> Result<(), anyhow::Error> {
        let network = MarketsNetwork::new("test_subscribe_offer")
            .add_market_instance("Node-1")
            .await?;

        let market1: Arc<MarketService> = network.get_market("Node-1");
        let identity1 = network.get_default_id("Node-1");

        let offer = Offer::new(json!({}), "()".to_string());
        let _subscription_id = market1.subscribe_offer(offer, identity1).await?;

        Ok(())
    }
}
