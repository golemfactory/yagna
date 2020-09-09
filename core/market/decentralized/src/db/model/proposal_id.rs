use chrono::NaiveDateTime;
use derive_more::Display;
use diesel::backend::Backend;
use diesel::deserialize::{FromSql, Result as DeserializeResult};
use diesel::serialize::{Output, Result as SerializeResult, ToSql};
use diesel::sql_types::Text;
use digest::Digest;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha3::Sha3_256;
use std::io::Write;
use std::str::FromStr;
use thiserror::Error;

use ya_client::model::ErrorMessage;

use crate::db::model::SubscriptionId;

#[derive(Display, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum OwnerType {
    #[display(fmt = "P")]
    Provider,
    #[display(fmt = "R")]
    Requestor,
}

#[derive(Error, Debug, PartialEq)]
pub enum ProposalIdParseError {
    #[error("Proposal id [{0}] has invalid format.")]
    InvalidFormat(String),
    #[error("Proposal id [{0}] has invalid owner type.")]
    InvalidOwnerType(String),
}

#[derive(thiserror::Error, Debug, PartialEq, Serialize, Deserialize)]
#[error("Proposal id [{0}] has unexpected hash [{1}].")]
pub struct ProposalIdValidationError(ProposalId, String);

#[derive(Display, Debug, Clone, AsExpression, FromSqlRow, Hash, PartialEq, Eq)]
#[display(fmt = "{}-{}", owner, id)]
#[sql_type = "Text"]
pub struct ProposalId {
    id: String,
    owner: OwnerType,
}

impl ProposalId {
    pub fn generate_id(
        offer_id: &SubscriptionId,
        demand_id: &SubscriptionId,
        creation_ts: &NaiveDateTime,
        owner: OwnerType,
    ) -> ProposalId {
        ProposalId {
            owner,
            id: hash_proposal(&offer_id, &demand_id, &creation_ts),
        }
    }

    pub fn owner(&self) -> OwnerType {
        self.owner.clone()
    }

    pub fn translate(mut self, new_owner: OwnerType) -> Self {
        self.owner = new_owner;
        self
    }

    pub fn swap_owner(mut self) -> Self {
        self.owner = match self.owner {
            OwnerType::Provider => OwnerType::Requestor,
            OwnerType::Requestor => OwnerType::Provider,
        };
        self
    }

    pub fn validate(
        &self,
        offer_id: &SubscriptionId,
        demand_id: &SubscriptionId,
        creation_ts: &NaiveDateTime,
    ) -> Result<(), ProposalIdValidationError> {
        let hash = hash_proposal(&offer_id, &demand_id, &creation_ts);
        if self.id != hash {
            return Err(ProposalIdValidationError(self.clone(), hash));
        }
        Ok(())
    }
}

pub fn hash_proposal(
    offer_id: &SubscriptionId,
    demand_id: &SubscriptionId,
    creation_ts: &NaiveDateTime,
) -> String {
    let mut hasher = Sha3_256::new();

    hasher.input(offer_id.to_string());
    hasher.input(demand_id.to_string());
    hasher.input(creation_ts.format("%Y-%m-%d %H:%M:%f").to_string());

    format!("{:x}", hasher.result())
}

impl FromStr for ProposalId {
    type Err = ProposalIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let elements: Vec<&str> = s.split('-').collect();

        if elements.len() != 2 {
            Err(ProposalIdParseError::InvalidFormat(s.to_string()))?;
        }

        if elements[0].len() != 1 {
            Err(ProposalIdParseError::InvalidOwnerType(s.to_string()))?;
        }

        let owner = match elements[0].chars().nth(0).unwrap() {
            'P' => OwnerType::Provider,
            'R' => OwnerType::Requestor,
            _ => Err(ProposalIdParseError::InvalidOwnerType(s.to_string()))?,
        };

        Ok(ProposalId {
            owner,
            id: elements[1].to_string(),
        })
    }
}

impl From<ProposalIdParseError> for ErrorMessage {
    fn from(e: ProposalIdParseError) -> Self {
        ErrorMessage::new(e.to_string())
    }
}

impl<DB> ToSql<Text, DB> for ProposalId
where
    DB: Backend,
    String: ToSql<Text, DB>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> SerializeResult {
        self.to_string().to_sql(out)
    }
}

impl<DB> FromSql<Text, DB> for ProposalId
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> DeserializeResult<Self> {
        Ok(String::from_sql(bytes)?.parse()?)
    }
}

impl Serialize for ProposalId {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ProposalId {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, <D as Deserializer<'de>>::Error> {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(de::Error::custom)
    }
}
