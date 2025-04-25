use alloy::primitives::{Address, B256 as AlloyB256};
use alloy_rlp::{RlpDecodable, RlpEncodable};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

use crate::client::GolemBaseClient;
use crate::entity::Annotation;

/// Type representing metadata for an entity.
#[derive(Debug, Clone, RlpEncodable, RlpDecodable, Serialize, Deserialize)]
pub struct EntityMetaData {
    /// The block number at which the entity expires.
    pub expires_at_block: u64,
    /// The payload associated with the entity.
    pub payload: String,
    /// String annotations for the entity.
    pub string_annotations: Vec<Annotation<String>>,
    /// Numeric annotations for the entity.
    pub numeric_annotations: Vec<Annotation<u64>>,
    /// The owner of the entity.
    pub owner: Address,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(rename = "key")]
    pub key: AlloyB256,
    #[serde(rename = "value")]
    pub value: Vec<u8>,
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
    pub async fn get_storage_value(&self, key: String) -> anyhow::Result<String> {
        let value = self
            .rpc_call::<&[String], String>("golembase_getStorageValue", &[key])
            .await?;
        // Decode base64 value
        let decoded = BASE64.decode(value)?;
        Ok(String::from_utf8(decoded)?)
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
        self.rpc_call::<AlloyB256, EntityMetaData>("golembase_getEntityMetaData", key)
            .await
    }
}
