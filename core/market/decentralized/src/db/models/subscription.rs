use chrono::NaiveDateTime;
use derive_more::Display;
use diesel::backend::Backend;
use diesel::deserialize::{FromSql, Result as DeserializeResult};
use diesel::serialize::{Output, Result as SerializeResult, ToSql};
use diesel::sql_types::Text;
use digest::Digest;
use serde::{Deserialize, Serialize};
use sha3::Sha3_256;
use std::io::Write;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

use digest::generic_array::GenericArray;
use ya_client::model::{ErrorMessage, NodeId};

#[derive(Error, Debug)]
pub enum SubscriptionParseError {
    #[error("Subscription id [{0}] has invalid format.")]
    InvalidFormat(String),
    #[error("Subscription id [{0}] contains non hexadecimal characters.")]
    NotHexadecimal(String),
    #[error("Subscription id [{0}] has invalid length.")]
    InvalidLength(String),
}

#[derive(
    Display, Debug, Clone, AsExpression, FromSqlRow, Hash, PartialEq, Eq, Serialize, Deserialize,
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
    ) -> Result<(), ErrorMessage> {
        let hash = hash(properties, constraints, node_id, creation_ts, expiration_ts);
        if self.hash != hash {
            Err(ErrorMessage::new(format!(
                "Invalid subscription id [{}]. Hash doesn't match content hash [{}].",
                &self, hash,
            )))?;
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
    hasher.input(creation_ts.format("%Y-%m-%d %H:%M:%S").to_string());
    hasher.input(expiration_ts.format("%Y-%m-%d %H:%M:%S").to_string());

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

        if elements[0].len() != 32 {
            Err(SubscriptionParseError::InvalidLength(s.to_string()))?;
        }

        if elements[1].len() != 64 {
            Err(SubscriptionParseError::InvalidLength(s.to_string()))?;
        }

        Ok(SubscriptionId {
            random_id: elements[0].to_string(),
            hash: elements[1].to_string(),
        })
    }
}

impl<DB> ToSql<Text, DB> for SubscriptionId
where
    DB: Backend,
    String: ToSql<Text, DB>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> SerializeResult {
        self.to_string().to_sql(out)
    }
}

impl<DB> FromSql<Text, DB> for SubscriptionId
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> DeserializeResult<Self> {
        let string = String::from_sql(bytes)?;
        match SubscriptionId::from_str(&string) {
            Ok(subscription) => Ok(subscription),
            Err(error) => Err(error.into()),
        }
    }
}

impl From<SubscriptionParseError> for ErrorMessage {
    fn from(err: SubscriptionParseError) -> Self {
        ErrorMessage::new(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_from_str() {
        let subscription_id = "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53";

        let sub_id = SubscriptionId::from_str(subscription_id).unwrap();
        assert_eq!(
            sub_id.hash.as_str(),
            "edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53"
        );
        assert_eq!(
            sub_id.random_id.as_str(),
            "c76161077d0343ab85ac986eb5f6ea38"
        );

        assert_eq!(SubscriptionId::from_str("34324-241").is_ok(), false);
        assert_eq!(SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53").is_ok(), false);
        assert_eq!(SubscriptionId::from_str("gfht-ertry").is_ok(), false);
    }
}
