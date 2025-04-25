use alloy::primitives::B256;
use alloy_rlp::{Encodable, RlpDecodable, RlpEncodable};
use bytes::Bytes;
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

/// Represents an annotation for an entity
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
pub struct Annotation<T> {
    pub key: String,
    pub value: T,
}

/// Represents a new entity creation operation
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
#[rlp(trailing)]
pub struct Create {
    pub ttl: u64,
    pub payload: Bytes,
    #[serde(rename = "stringAnnotations")]
    #[rlp(rename = "stringAnnotations")]
    strings: Vec<Annotation<String>>,
    #[serde(rename = "numericAnnotations")]
    #[rlp(rename = "numericAnnotations")]
    numbers: Vec<Annotation<u64>>,
}

impl Create {
    /// Creates a new Create operation with empty annotations
    pub fn new(payload: Vec<u8>, ttl: u64) -> Self {
        Self {
            ttl,
            payload: Bytes::from(payload),
            strings: Vec::new(),
            numbers: Vec::new(),
        }
    }

    /// Adds a string annotation to the entity
    pub fn annotate_string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.strings.push(Annotation {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Adds a numeric annotation to the entity
    pub fn annotate_number(mut self, key: impl Into<String>, value: u64) -> Self {
        self.numbers.push(Annotation {
            key: key.into(),
            value,
        });
        self
    }
}

/// Represents an entity update operation
#[derive(Debug, Clone, Serialize, Deserialize, RlpEncodable, RlpDecodable)]
#[rlp(trailing)]
pub struct Update {
    #[serde(rename = "entityKey")]
    #[rlp(rename = "entityKey")]
    pub entity_key: B256,
    pub ttl: u64,
    pub payload: Bytes,
    #[serde(rename = "stringAnnotations")]
    #[rlp(rename = "stringAnnotations")]
    strings: Vec<Annotation<String>>,
    #[serde(rename = "numericAnnotations")]
    #[rlp(rename = "numericAnnotations")]
    numbers: Vec<Annotation<u64>>,
}

impl Update {
    /// Creates a new Update operation with empty annotations
    pub fn new(entity_key: B256, payload: Vec<u8>, ttl: u64) -> Self {
        Self {
            entity_key,
            ttl,
            payload: Bytes::from(payload),
            strings: Vec::new(),
            numbers: Vec::new(),
        }
    }

    /// Adds a string annotation to the entity
    pub fn annotate_string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.strings.push(Annotation {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Adds a numeric annotation to the entity
    pub fn annotate_number(mut self, key: impl Into<String>, value: u64) -> Self {
        self.numbers.push(Annotation {
            key: key.into(),
            value,
        });
        self
    }
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

impl StorageTransaction {
    /// Returns the RLP-encoded bytes of the transaction
    pub fn encoded(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        self.encode(&mut encoded);
        encoded
    }
}

// Tests check serialization compatibility with go implementation.
#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::B256;
    use hex;

    #[test]
    fn test_empty_transaction() {
        let tx = StorageTransaction::default();
        assert_eq!(hex::encode(tx.encoded()), "c4c0c0c0c0");
    }

    #[test]
    fn test_create_without_annotations() {
        let create = Create::new(b"test payload".to_vec(), 1000);

        let mut tx = StorageTransaction::default();
        tx.create.push(create);

        assert_eq!(
            hex::encode(tx.encoded()),
            "d7d3d28203e88c74657374207061796c6f6164c0c0c0c0c0"
        );
    }

    #[test]
    fn test_create_with_annotations() {
        let create = Create::new(b"test payload".to_vec(), 1000)
            .annotate_string("foo", "bar")
            .annotate_number("baz", 42);

        let mut tx = StorageTransaction::default();
        tx.create.push(create);

        assert_eq!(
            hex::encode(tx.encoded()),
            "e6e2e18203e88c74657374207061796c6f6164c9c883666f6f83626172c6c58362617a2ac0c0c0"
        );
    }

    #[test]
    fn test_update_with_annotations() {
        let update = Update::new(
            B256::from_slice(&[1; 32]),
            b"updated payload".to_vec(),
            2000,
        )
        .annotate_string("status", "active")
        .annotate_number("version", 2);

        let mut tx = StorageTransaction::default();
        tx.update.push(update);

        assert_eq!(
            hex::encode(tx.encoded()),
            "f856c0f851f84fa001010101010101010101010101010101010101010101010101010101010101018207d08f75706461746564207061796c6f6164cfce8673746174757386616374697665cac98776657273696f6e02c0c0"
        );
    }

    #[test]
    fn test_delete_operation() {
        let mut tx = StorageTransaction::default();
        tx.delete.push(B256::from_slice(&[2; 32]));

        assert_eq!(
            hex::encode(tx.encoded()),
            "e5c0c0e1a00202020202020202020202020202020202020202020202020202020202020202c0"
        );
    }

    #[test]
    fn test_extend_ttl() {
        let mut tx = StorageTransaction::default();
        tx.extend.push(ExtendTTL {
            entity_key: B256::from_slice(&[3; 32]),
            number_of_blocks: 500,
        });

        assert_eq!(
            hex::encode(tx.encoded()),
            "e9c0c0c0e5e4a003030303030303030303030303030303030303030303030303030303030303038201f4"
        );
    }

    #[test]
    fn test_mixed_operations() {
        let create = Create::new(b"test payload".to_vec(), 1000).annotate_string("type", "test");
        let update = Update::new(
            B256::from_slice(&[1; 32]),
            b"updated payload".to_vec(),
            2000,
        );
        let mut tx = StorageTransaction::default();
        tx.create.push(create);
        tx.update.push(update);
        tx.delete.push(B256::from_slice(&[2; 32]));
        tx.extend.push(ExtendTTL {
            entity_key: B256::from_slice(&[3; 32]),
            number_of_blocks: 500,
        });

        assert_eq!(
            hex::encode(tx.encoded()),
            "f89fdedd8203e88c74657374207061796c6f6164cbca84747970658474657374c0f7f6a001010101010101010101010101010101010101010101010101010101010101018207d08f75706461746564207061796c6f6164c0c0e1a00202020202020202020202020202020202020202020202020202020202020202e5e4a003030303030303030303030303030303030303030303030303030303030303038201f4"
        );
    }
}
