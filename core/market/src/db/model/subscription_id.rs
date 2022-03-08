use chrono::NaiveDateTime;
use diesel::sql_types::Text;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha3::{Digest, Sha3_256};
use std::str::FromStr;
use uuid::Uuid;

use ya_client::model::{ErrorMessage, NodeId};
use ya_diesel_utils::DbTextField;

const RANDOM_PREFIX_LEN: usize = 32;
const HASH_SUFFIX_LEN: usize = 64;

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum SubscriptionParseError {
    #[error("Subscription id [{0}] has invalid format.")]
    InvalidFormat(String),
    #[error("Subscription id [{0}] contains non hexadecimal characters.")]
    NotHexadecimal(String),
    #[error(
        "Subscription id [{0}] has invalid length. Should be |{}|-|{}|",
        RANDOM_PREFIX_LEN,
        HASH_SUFFIX_LEN
    )]
    InvalidLength(String),
}

#[derive(thiserror::Error, Debug, PartialEq)]
#[error("Subscription id [{0}] doesn't match content hash [{1}].")]
pub struct SubscriptionValidationError(SubscriptionId, String);

#[derive(
    DbTextField, derive_more::Display, Debug, Clone, AsExpression, FromSqlRow, Hash, PartialEq, Eq,
)]
#[display(fmt = "{}-{}", random_id, hash)]
#[sql_type = "Text"]
pub struct SubscriptionId {
    random_id: String,
    hash: String,
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
            random_id: generate_random_id(),
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
            return Err(SubscriptionValidationError(self.clone(), hash));
        }
        Ok(())
    }
}

pub fn hash(
    properties: &str,
    constraints: &str,
    node_id: &NodeId,
    creation_ts: &NaiveDateTime,
    expiration_ts: &NaiveDateTime,
) -> String {
    let mut hasher = Sha3_256::new();

    hasher.input(properties);
    hasher.input(constraints);
    hasher.input(node_id);
    // We can't change format freely, because it is important to compute hash.
    // Is there any other solution, to compute hash, that is format independent?
    hasher.input(creation_ts.format("%Y-%m-%d %H:%M:%f").to_string());
    hasher.input(expiration_ts.format("%Y-%m-%d %H:%M:%f").to_string());

    format!("{:x}", hasher.result())
}

impl FromStr for SubscriptionId {
    type Err = SubscriptionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let elements: Vec<&str> = s.split('-').collect();

        if elements.len() != 2 {
            Err(SubscriptionParseError::InvalidFormat(s.to_string()))?;
        }

        if !elements
            .iter()
            .map(|slice| slice.chars().all(|character| character.is_ascii_hexdigit()))
            .all(|result| result == true)
        {
            Err(SubscriptionParseError::NotHexadecimal(s.to_string()))?;
        }

        if elements[0].len() != RANDOM_PREFIX_LEN {
            Err(SubscriptionParseError::InvalidLength(s.to_string()))?;
        }

        if elements[1].len() != HASH_SUFFIX_LEN {
            Err(SubscriptionParseError::InvalidLength(s.to_string()))?;
        }

        Ok(SubscriptionId {
            random_id: elements[0].to_string(),
            hash: elements[1].to_string(),
        })
    }
}

impl Serialize for SubscriptionId {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error> {
        serializer.serialize_str(&self.to_string())
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
        InvalidFormat, InvalidLength, NotHexadecimal,
    };
    use chrono::NaiveDate;

    #[test]
    fn should_parse_subscription_id() {
        let subscription_id = "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53";

        let sub_id = SubscriptionId::from_str(subscription_id).unwrap();
        assert_eq!(
            &sub_id.hash,
            "edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53"
        );
        assert_eq!(&sub_id.random_id, "c76161077d0343ab85ac986eb5f6ea38");
    }

    #[test]
    fn should_be_case_sensitive_subscription_id() {
        assert_ne!(
            SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53").unwrap(),
            SubscriptionId::from_str("C76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53").unwrap(),
        );
    }

    #[test]
    fn should_fail_to_parse_subscription_id() {
        assert_eq!(SubscriptionId::from_str(""), Err(InvalidFormat("".into())));
        assert_eq!(
            SubscriptionId::from_str("a"),
            Err(InvalidFormat("a".into()))
        );
        assert_eq!(
            SubscriptionId::from_str("x-x"),
            Err(NotHexadecimal("x-x".into()))
        );
        assert_eq!(
            SubscriptionId::from_str("gfht-ertry"),
            Err(NotHexadecimal("gfht-ertry".into()))
        );
        let invalid_len = SubscriptionId::from_str("34324-241");
        assert_eq!(invalid_len, Err(InvalidLength("34324-241".into())));
        assert_eq!(
            invalid_len.unwrap_err().to_string(),
            "Subscription id [34324-241] has invalid length. Should be |32|-|64|"
        );

        assert_eq!(
            SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-"),
            Err(InvalidLength("c76161077d0343ab85ac986eb5f6ea38-".into()))
        );
        assert_eq!(
            SubscriptionId::from_str(
                "-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53"
            ),
            Err(InvalidLength(
                "-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53".into()
            ))
        );
        assert_eq!(
            SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38F-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53"),
            Err(InvalidLength("c76161077d0343ab85ac986eb5f6ea38F-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53".into()))
        );
        assert_eq!(
            SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53F"),
            Err(InvalidLength("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53F".into()))
        );
        assert_eq!(
            SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38F-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53F"),
            Err(InvalidLength("c76161077d0343ab85ac986eb5f6ea38F-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53F".into()))
        );
    }

    #[test]
    fn should_validate() {
        let properties = "{}";
        let constraints = "()";
        let node_id = NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap();
        let creation_ts = NaiveDate::from_ymd(2020, 6, 19).and_hms(18, 53, 1);
        let expiration_ts = NaiveDate::from_ymd(2020, 6, 19).and_hms(20, 19, 17);
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
        let bad_subscription_id = SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53").unwrap();
        let properties = "{}";
        let constraints = "()";
        let node_id = NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap();
        let creation_ts = NaiveDate::from_ymd(2020, 6, 19).and_hms(18, 53, 1);
        let expiration_ts = NaiveDate::from_ymd(2020, 6, 19).and_hms(20, 19, 17);
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
                hash(
                    properties,
                    constraints,
                    &node_id,
                    &creation_ts,
                    &expiration_ts
                )
            ))
        );
    }
}
