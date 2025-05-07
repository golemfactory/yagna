use alloy::primitives::{Address, B256 as AlloyB256};
use alloy_rlp::{RlpDecodable, RlpEncodable};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

use crate::client::GolemBaseClient;
use crate::entity::Annotation;

/// Type representing metadata for an entity.
#[derive(Debug, Clone, RlpEncodable, RlpDecodable, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityMetaData {
    /// The block number at which the entity expires.
    #[serde(rename = "expiresAtBlock")]
    #[rlp(rename = "expiresAtBlock")]
    pub expires_at_block: u64,
    /// String annotations for the entity.
    #[serde(rename = "stringAnnotations")]
    #[rlp(rename = "stringAnnotations")]
    pub string_annotations: Vec<Annotation<String>>,
    /// Numeric annotations for the entity.
    #[serde(rename = "numericAnnotations")]
    #[rlp(rename = "numericAnnotations")]
    pub numeric_annotations: Vec<Annotation<u64>>,
    /// The owner of the entity.
    #[serde(rename = "owner")]
    #[rlp(rename = "owner")]
    pub owner: Address,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(rename = "key")]
    pub key: AlloyB256,
    #[serde(rename = "value", deserialize_with = "deserialize_base64")]
    pub value: Bytes,
}

impl SearchResult {
    /// Converts the value to a UTF-8 string
    pub fn value_as_string(&self) -> anyhow::Result<String> {
        String::from_utf8(self.value.to_vec())
            .map_err(|e| anyhow::anyhow!("Failed to decode search result to string: {}", e))
    }
}

fn deserialize_base64<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    BASE64
        .decode(s)
        .map(Bytes::from)
        .map_err(serde::de::Error::custom)
}

impl GolemBaseClient {
    /// Gets the total count of entities in GolemBase.
    pub async fn get_entity_count(&self) -> anyhow::Result<u64> {
        self.rpc_call::<(), u64>("golembase_getEntityCount", ())
            .await
    }

    /// Gets the entity keys of all entities in GolemBase.
    pub async fn get_all_entity_keys(&self) -> anyhow::Result<Vec<AlloyB256>> {
        self.rpc_call::<(), Vec<AlloyB256>>("golembase_getAllEntityKeys", ())
            .await
    }

    /// Gets the entity keys of all entities owned by the given address.
    pub async fn get_entities_of_owner(&self, address: Address) -> anyhow::Result<Vec<AlloyB256>> {
        self.rpc_call::<&[Address], Vec<AlloyB256>>("golembase_getEntitiesOfOwner", &[address])
            .await
    }

    /// Gets the storage value associated with the given entity key.
    pub async fn get_storage_value(&self, key: String) -> anyhow::Result<Bytes> {
        let value = self
            .rpc_call::<&[String], String>("golembase_getStorageValue", &[key])
            .await?;
        // Decode base64 value
        Ok(Bytes::from(BASE64.decode(value)?))
    }

    /// Gets the storage value as a string associated with the given entity key.
    pub async fn get_storage_value_string(&self, key: String) -> anyhow::Result<String> {
        let bytes = self.get_storage_value(key).await?;
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    /// Queries entities in GolemBase based on annotations.
    pub async fn query_entities(&self, query: &str) -> anyhow::Result<Vec<SearchResult>> {
        self.rpc_call::<&[&str], Vec<SearchResult>>("golembase_queryEntities", &[query])
            .await
    }

    /// Gets all entities with a given string annotation.
    pub async fn get_entities_for_string_annotation_value(
        &self,
        key: AlloyB256,
        value: String,
    ) -> anyhow::Result<Vec<AlloyB256>> {
        let params = Annotation {
            key: key.to_string(),
            value,
        };
        self.rpc_call::<Annotation<String>, Vec<AlloyB256>>(
            "golembase_getEntitiesForStringAnnotationValue",
            params,
        )
        .await
    }

    /// Gets all entities with a given numeric annotation.
    pub async fn get_entities_for_numeric_annotation_value(
        &self,
        key: AlloyB256,
        value: u64,
    ) -> anyhow::Result<Vec<AlloyB256>> {
        let params = Annotation {
            key: key.to_string(),
            value,
        };
        self.rpc_call::<Annotation<u64>, Vec<AlloyB256>>(
            "golembase_getEntitiesForNumericAnnotationValue",
            params,
        )
        .await
    }

    /// Gets all entity keys for entities that will expire at the given block number.
    pub async fn get_entities_to_expire_at_block(
        &self,
        block_number: u64,
    ) -> anyhow::Result<Vec<AlloyB256>> {
        self.rpc_call::<u64, Vec<AlloyB256>>("golembase_getEntitiesToExpireAtBlock", block_number)
            .await
    }

    /// Gets metadata for a specific entity.
    pub async fn get_entity_metadata(&self, key: AlloyB256) -> anyhow::Result<EntityMetaData> {
        self.rpc_call::<&[AlloyB256], EntityMetaData>("golembase_getEntityMetaData", &[key])
            .await
    }
}
