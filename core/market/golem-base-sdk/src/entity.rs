use alloy::primitives::B256;
use alloy_rlp::{RlpDecodable, RlpEncodable};
use serde::{Deserialize, Serialize};

/// Represents a storage transaction containing multiple operations
#[derive(Debug, Clone, Default, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct StorageTransaction {
    #[serde(default)]
    #[rlp(default)]
    pub create: Vec<Create>,
    #[serde(default)]
    #[rlp(default)]
    pub update: Vec<Update>,
    #[serde(default)]
    #[rlp(default)]
    pub delete: Vec<B256>,
    #[serde(default)]
    #[rlp(default)]
    pub extend: Vec<ExtendTTL>,
}

/// Helper struct for managing entity annotations
#[derive(Debug, Clone, Default, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct Annotations {
    #[serde(default)]
    #[rlp(default)]
    #[serde(rename = "stringAnnotations")]
    #[rlp(rename = "stringAnnotations")]
    strings: Vec<StringAnnotation>,
    #[serde(default)]
    #[rlp(default)]
    #[serde(rename = "numericAnnotations")]
    #[rlp(rename = "numericAnnotations")]
    numbers: Vec<NumericAnnotation>,
}

/// Represents a new entity creation operation
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct Create {
    pub ttl: u64,
    pub payload: Vec<u8>,
    #[serde(flatten)]
    #[rlp(flatten)]
    annotations: Annotations,
}

/// Represents an entity update operation
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct Update {
    #[serde(rename = "entityKey")]
    #[rlp(rename = "entityKey")]
    pub entity_key: B256,
    pub ttl: u64,
    pub payload: Vec<u8>,
    #[serde(flatten)]
    #[rlp(flatten)]
    annotations: Annotations,
}

/// Represents a TTL extension operation
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct ExtendTTL {
    #[serde(rename = "entityKey")]
    #[rlp(rename = "entityKey")]
    pub entity_key: B256,
    #[serde(rename = "numberOfBlocks")]
    #[rlp(rename = "numberOfBlocks")]
    pub number_of_blocks: u64,
}

/// Represents a string annotation for an entity
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct StringAnnotation {
    pub key: String,
    pub value: String,
}

/// Represents a numeric annotation for an entity
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct NumericAnnotation {
    pub key: String,
    pub value: u64,
}

impl Annotations {
    /// Adds a string annotation
    pub fn annotate_string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.strings.push(StringAnnotation {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Adds a numeric annotation
    pub fn annotate_number(mut self, key: impl Into<String>, value: u64) -> Self {
        self.numbers.push(NumericAnnotation {
            key: key.into(),
            value,
        });
        self
    }
}

impl Create {
    /// Creates a new Create operation with empty annotations
    pub fn new(payload: Vec<u8>, ttl: u64) -> Self {
        Self {
            ttl,
            payload,
            annotations: Annotations::default(),
        }
    }

    /// Adds a string annotation to the entity
    pub fn annotate_string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations = self.annotations.annotate_string(key, value);
        self
    }

    /// Adds a numeric annotation to the entity
    pub fn annotate_number(mut self, key: impl Into<String>, value: u64) -> Self {
        self.annotations = self.annotations.annotate_number(key, value);
        self
    }
}

impl Update {
    /// Creates a new Update operation with empty annotations
    pub fn new(entity_key: B256, payload: Vec<u8>, ttl: u64) -> Self {
        Self {
            entity_key,
            ttl,
            payload,
            annotations: Annotations::default(),
        }
    }

    /// Adds a string annotation to the entity
    pub fn annotate_string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations = self.annotations.annotate_string(key, value);
        self
    }

    /// Adds a numeric annotation to the entity
    pub fn annotate_number(mut self, key: impl Into<String>, value: u64) -> Self {
        self.annotations = self.annotations.annotate_number(key, value);
        self
    }
}
