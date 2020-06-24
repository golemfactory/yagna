use async_trait::async_trait;
use std::str::FromStr;
use std::string::ToString;
use std::sync::Arc;
use std::time::Duration;

use ya_client::model::market::{Demand, Offer};
use ya_market_decentralized::testing::DraftProposal;
use ya_market_decentralized::testing::SubscriptionId;
use ya_market_decentralized::testing::{DemandDao, OfferDao};
use ya_market_decentralized::MarketService;

use crate::utils::MarketsNetwork;
use crate::utils::mock_node::MarketNode;


#[allow(unused)]
pub fn example_offer() -> Offer {
    let properties = serde_json::json!({
        "golem": {
            "golem.node.debug.subnet": "blaa".to_string(),
            "node.id.name": "itstest".to_string(),
            "srv.comp.wasm.task_package": "test-package".to_string(),
        },
    });
    Offer::new(properties, "(golem.node.debug.subnet=blaa)".to_string())
}

#[allow(unused)]
pub fn example_demand() -> Demand {
    let properties = serde_json::json!({
        "golem": {
            "golem.node.debug.subnet": "blaa".to_string(),
            "node.id.name": "itstest".to_string(),
            "srv.comp.wasm.task_package": "test-package".to_string(),
        },
    });
    Demand::new(properties, "(golem.node.debug.subnet=blaa)".to_string())
}

impl MarketNode {
    pub async fn inject_proposal(
        &self,
        offer: &Offer,
        demand: &Demand,
    ) -> Result<(SubscriptionId, SubscriptionId), anyhow::Error> {
        let market1: Arc<MarketService> = self.market.clone();
        let identity1 = self.identity.clone();

        let subscription_id = market1.subscribe_demand(demand, identity1).await?;
        let subscription_id = SubscriptionId::from_str(&subscription_id)?;

        self.inject_proposal_for_demand(offer, &subscription_id).await
    }

    pub async fn inject_proposal_for_demand(
        &self,
        offer: &Offer,
        demand_id: &SubscriptionId,
    ) -> Result<(SubscriptionId, SubscriptionId), anyhow::Error> {
        let market1: Arc<MarketService> = self.market.clone();
        let identity1 = self.identity.clone();

        // We need model Offer. So we will get it from database.
        let offer_id = market1.subscribe_offer(offer, identity1.clone()).await?;

        // Get model Demand to directly inject it into negotiation engine.
        let db = self.db.clone();
        let model_demand = db
            .as_dao::<DemandDao>()
            .get_demand(&demand_id)
            .await?
            .unwrap();

        let model_offer = db
            .as_dao::<OfferDao>()
            .get_offer(&SubscriptionId::from_str(offer_id.as_ref())?)
            .await?
            .unwrap();

        let proposal = DraftProposal {
            offer: model_offer,
            demand: model_demand,
        };
        market1.matcher.emit_proposal(proposal)?;
        tokio::time::delay_for(Duration::from_millis(30)).await;

        let offer_id = SubscriptionId::from_str(offer_id.as_ref())?;
        Ok((offer_id, demand_id.clone()))
    }
}
