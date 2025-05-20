use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use golem_base_sdk::Hash;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{db::model::Offer as ModelOffer, testing::SubscriptionId};

use ya_agreement_utils::agreement::{expand, flatten};
use ya_client::model::{market::NewOffer, NodeId};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GolemBaseOffer {
    #[serde(rename = "properties")]
    pub properties: Value,
    #[serde(rename = "constraints")]
    pub constraints: String,
    #[serde(rename = "providerId")]
    pub provider_id: NodeId,
    #[serde(rename = "expiration")]
    pub expiration: DateTime<Utc>,
    #[serde(rename = "timestamp")]
    pub timestamp: DateTime<Utc>,
}

impl GolemBaseOffer {
    pub fn create(offer: &NewOffer, id: NodeId, default_ttl: Duration) -> Self {
        let creation_ts = Utc::now();
        let expiration_ts = creation_ts + default_ttl;

        // Properties are always in expanded format.
        Self {
            properties: expand(offer.properties.clone()),
            constraints: offer.constraints.clone(),
            provider_id: id,
            expiration: expiration_ts,
            timestamp: creation_ts,
        }
    }

    pub fn into_model_offer(self, key: Hash) -> Result<ModelOffer> {
        // ModelOffer properties are always in flattened format.
        let properties = serde_json::to_string(&flatten(self.properties))
            .map_err(|e| anyhow::anyhow!("Failed to serialize properties: {}", e))?;
        let creation_ts = self.timestamp.naive_utc();
        let expiration_ts = self.expiration.naive_utc();

        let id = SubscriptionId::from_bytes(key.0);
        Ok(ModelOffer {
            id,
            properties,
            constraints: self.constraints,
            node_id: self.provider_id,
            creation_ts,
            insertion_ts: None,
            expiration_ts,
        })
    }
}
