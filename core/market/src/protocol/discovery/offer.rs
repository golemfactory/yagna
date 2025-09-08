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

    /// Calculate TTL in blocks based on expiration time
    ///
    /// GolemBase allows only for quantized expiration every `block_time_seconds`.
    /// This means that we always have situation when Offer expiration is not exactly in sync with
    /// entry block expiration and we must make decision in which direction to round it.
    /// We have 3 options:
    /// - Adjust model Offer expiration timestamp to the quantiazed value from GolemBase.
    /// - Round down number of blocks to expire Offer earlier on GolemBase than in internal market implementation.
    /// - Round up number of blocksto expire Offer later on GolemBase than in internal market implementation.
    ///
    /// First option is dangerous if user would ever have option to set expiration time manually, because
    /// it means that the value would change without user's knowledge.
    ///
    /// Rounding down would mean that we will get message from GolemBase about Offer removal, before
    /// expiration timestamp will elapse and the Offer will be marked as unsubscribed in database, despite
    /// user didn't do it.
    ///
    /// For those reasons it seem rounding up is the only option that makes sense.
    /// The other concern we must take into account is that we don't really know in which block the entity
    /// will be included, so rounding down would sometimes result in the same scenario as rounding up.
    pub fn calculate_ttl_blocks(&self, block_time_seconds: i64) -> u64 {
        let ttl_seconds = (self.expiration - self.timestamp).as_seconds_f64().ceil();
        if ttl_seconds <= 0.0 {
            0
        } else {
            let ttl_blocks = ttl_seconds / (block_time_seconds as f64);
            // We give 1 block expiration margin in case we are on the fence.
            ttl_blocks.ceil() as u64
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
            owned: None,
        })
    }
}
