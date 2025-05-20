use chrono::NaiveDateTime;
use diesel::sql_types::Text;
use digest::Digest;
use hex;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha3::Sha3_256;
use std::{fmt::Display, str::FromStr};
use uuid::Uuid;

use ya_client::model::{ErrorMessage, NodeId};
use ya_diesel_utils::DbTextField;

pub const HASH_LEN: usize = 64;
pub const HASH_BYTES_LEN: usize = 32;

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum SubscriptionParseError {
    #[error("Subscription id [{0}] has invalid format.")]
    InvalidFormat(String),
    #[error("Subscription id [{0}] contains non hexadecimal characters.")]
    NotHexadecimal(String),
    #[error("Subscription id [{0}] has invalid length. Should be |{}|", HASH_LEN)]
    InvalidLength(String),
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
#[error("Subscription id [{0}] doesn't match content hash [{1}].")]
pub struct SubscriptionValidationError(SubscriptionId, String);

#[derive(DbTextField, Debug, Clone, AsExpression, FromSqlRow, Hash, PartialEq, Eq)]
#[sql_type = "Text"]
pub struct SubscriptionId {
    hash: [u8; HASH_BYTES_LEN],
}

/// TODO: Should be cryptographically strong.
pub fn generate_random_id() -> String {
    Uuid::new_v4().to_simple().to_string()
}

impl SubscriptionId {
    pub fn generate_id(
        properties: &str,
        constraints: &str,
        node_id: &NodeId,
        creation_ts: &NaiveDateTime,
        expiration_ts: &NaiveDateTime,
    ) -> SubscriptionId {
        SubscriptionId {
            hash: hash(properties, constraints, node_id, creation_ts, expiration_ts),
        }
    }

    pub fn validate(
        &self,
        properties: &str,
        constraints: &str,
        node_id: &NodeId,
        creation_ts: &NaiveDateTime,
        expiration_ts: &NaiveDateTime,
    ) -> Result<(), SubscriptionValidationError> {
        let hash = hash(properties, constraints, node_id, creation_ts, expiration_ts);
        if self.hash != hash {
            return Err(SubscriptionValidationError(self.clone(), hex::encode(hash)));
        }
        Ok(())
    }

    /// Converts the subscription ID hash to a byte array.
    /// Returns a 32-byte array representing the SHA3-256 hash.
    pub fn to_bytes(&self) -> [u8; HASH_BYTES_LEN] {
        self.hash
    }

    /// Creates a SubscriptionId from a byte array.
    /// The input should be a 32-byte array representing a SHA3-256 hash.
    pub fn from_bytes(bytes: [u8; HASH_BYTES_LEN]) -> Self {
        SubscriptionId { hash: bytes }
    }
}

fn hash(
    properties: &str,
    constraints: &str,
    node_id: &NodeId,
    creation_ts: &NaiveDateTime,
    expiration_ts: &NaiveDateTime,
) -> [u8; 32] {
    // Canonicalize properties. They are already serialized in `properties` variable,
    // so we need to deserialize them first.
    let properties: serde_json::Value =
        serde_json::from_str(properties).unwrap_or_else(|_| serde_json::from_str("{}").unwrap());
    let properties = serde_json_canonicalizer::to_vec(&properties).unwrap();

    let mut hasher = Sha3_256::new();

    hasher.input(properties);
    hasher.input(constraints);
    hasher.input(node_id);
    // We can't change format freely, because it is important to compute hash.
    // Is there any other solution, to compute hash, that is format independent?
    hasher.input(creation_ts.format("%Y-%m-%d %H:%M:%f").to_string());
    hasher.input(expiration_ts.format("%Y-%m-%d %H:%M:%f").to_string());

    let mut result = [0u8; 32];
    result.copy_from_slice(&hasher.result());
    result
}

impl FromStr for SubscriptionId {
    type Err = SubscriptionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.chars().all(|character| character.is_ascii_hexdigit()) {
            Err(SubscriptionParseError::NotHexadecimal(s.to_string()))?;
        }

        if s.len() != HASH_LEN {
            Err(SubscriptionParseError::InvalidLength(s.to_string()))?;
        }

        let mut bytes = [0u8; 32];
        hex::decode_to_slice(s, &mut bytes)
            .map_err(|_| SubscriptionParseError::NotHexadecimal(s.to_string()))?;

        Ok(SubscriptionId { hash: bytes })
    }
}

impl Display for SubscriptionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.hash))
    }
}

impl Serialize for SubscriptionId {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error> {
        serializer.serialize_str(&hex::encode(self.hash))
    }
}

impl<'de> Deserialize<'de> for SubscriptionId {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, <D as Deserializer<'de>>::Error> {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(de::Error::custom)
    }
}

impl From<SubscriptionParseError> for ErrorMessage {
    fn from(e: SubscriptionParseError) -> Self {
        ErrorMessage::new(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::model::subscription_id::SubscriptionParseError::{
        InvalidLength, NotHexadecimal,
    };
    use chrono::NaiveDate;

    #[test]
    fn should_parse_subscription_id() {
        let subscription_id = "edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53";

        let sub_id = SubscriptionId::from_str(subscription_id).unwrap();
        assert_eq!(
            &sub_id.to_string(),
            "edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53"
        );
    }

    #[test]
    fn should_not_be_case_sensitive_subscription_id() {
        assert_eq!(
            SubscriptionId::from_str(
                "edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53"
            )
            .unwrap(),
            SubscriptionId::from_str(
                "EDB0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53"
            )
            .unwrap(),
        );
    }

    #[test]
    fn should_fail_to_parse_subscription_id() {
        assert_eq!(SubscriptionId::from_str(""), Err(InvalidLength("".into())));
        assert_eq!(
            SubscriptionId::from_str("x"),
            Err(NotHexadecimal("x".into()))
        );
        assert_eq!(
            SubscriptionId::from_str("gfht"),
            Err(NotHexadecimal("gfht".into()))
        );
        let invalid_len = SubscriptionId::from_str("34324");
        assert_eq!(invalid_len, Err(InvalidLength("34324".into())));
        assert_eq!(
            invalid_len.unwrap_err().to_string(),
            "Subscription id [34324] has invalid length. Should be |64|"
        );
    }

    #[test]
    fn should_validate() {
        let properties = "{}";
        let constraints = "()";
        let node_id = NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap();
        let creation_ts = NaiveDate::from_ymd_opt(2020, 6, 19)
            .unwrap()
            .and_hms_opt(18, 53, 1)
            .unwrap();
        let expiration_ts = NaiveDate::from_ymd_opt(2020, 6, 19)
            .unwrap()
            .and_hms_opt(20, 19, 17)
            .unwrap();
        let good_subscription_id = SubscriptionId::generate_id(
            properties,
            constraints,
            &node_id,
            &creation_ts,
            &expiration_ts,
        );
        assert_eq!(
            good_subscription_id.validate(
                properties,
                constraints,
                &node_id,
                &creation_ts,
                &expiration_ts
            ),
            Ok(())
        );
    }

    #[test]
    fn should_not_validate() {
        let bad_subscription_id = SubscriptionId::from_str(
            "edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",
        )
        .unwrap();
        let properties = "{}";
        let constraints = "()";
        let node_id = NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap();
        let creation_ts = NaiveDate::from_ymd_opt(2020, 6, 19)
            .unwrap()
            .and_hms_opt(18, 53, 1)
            .unwrap();
        let expiration_ts = NaiveDate::from_ymd_opt(2020, 6, 19)
            .unwrap()
            .and_hms_opt(20, 19, 17)
            .unwrap();
        assert_eq!(
            bad_subscription_id.validate(
                properties,
                constraints,
                &node_id,
                &creation_ts,
                &expiration_ts
            ),
            Err(SubscriptionValidationError(
                bad_subscription_id,
                hex::encode(hash(
                    properties,
                    constraints,
                    &node_id,
                    &creation_ts,
                    &expiration_ts
                ))
            ))
        );
    }

    #[test]
    fn should_convert_to_and_from_bytes() {
        let subscription_id = "edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53";
        let sub_id = SubscriptionId::from_str(subscription_id).unwrap();

        let bytes = sub_id.to_bytes();
        let reconstructed = SubscriptionId::from_bytes(bytes);

        assert_eq!(sub_id, reconstructed);
    }

    #[test]
    fn should_convert_to_and_from_bytes_with_generated_id() {
        let properties = "{}";
        let constraints = "()";
        let node_id = NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap();
        let creation_ts = NaiveDate::from_ymd_opt(2020, 6, 19)
            .unwrap()
            .and_hms_opt(18, 53, 1)
            .unwrap();
        let expiration_ts = NaiveDate::from_ymd_opt(2020, 6, 19)
            .unwrap()
            .and_hms_opt(20, 19, 17)
            .unwrap();

        let sub_id = SubscriptionId::generate_id(
            properties,
            constraints,
            &node_id,
            &creation_ts,
            &expiration_ts,
        );

        let bytes = sub_id.to_bytes();
        let reconstructed = SubscriptionId::from_bytes(bytes);

        assert_eq!(sub_id, reconstructed);
    }
}
