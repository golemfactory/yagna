use chrono::NaiveDateTime;
use derive_more::Display;
use diesel::sql_types::Text;
use digest::Digest;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha3::Sha3_256;
use std::str::FromStr;
use thiserror::Error;

use ya_diesel_utils::DbTextField;

use crate::db::model::SubscriptionId;

#[derive(
    DbTextField,
    Display,
    Debug,
    Clone,
    Copy,
    PartialEq,
    AsExpression,
    FromSqlRow,
    Eq,
    Serialize,
    Deserialize,
    Hash,
)]
#[sql_type = "Text"]
pub enum Owner {
    #[display(fmt = "P")]
    Provider,
    #[display(fmt = "R")]
    Requestor,
}

impl Owner {
    pub fn swap(self) -> Self {
        match self {
            Self::Provider => Self::Requestor,
            Self::Requestor => Self::Provider,
        }
    }
}

const HASH_SUFFIX_LEN: usize = 64;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum ProposalIdParseError {
    #[error("Id [{0}] has invalid format.")]
    InvalidFormat(String),
    #[error("Id [{0}] has invalid owner type.")]
    InvalidOwner(String),
    #[error("Id [{0}] contains non hexadecimal characters.")]
    NotHexadecimal(String),
    #[error("Id [{0}] hash has invalid length. Should be |{}|", HASH_SUFFIX_LEN)]
    InvalidLength(String),
}

#[derive(thiserror::Error, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[error("Proposal id [{0}] has unexpected hash [{1}].")]
pub struct ProposalIdValidationError(ProposalId, String);

#[derive(DbTextField, Display, Debug, Clone, AsExpression, FromSqlRow, Hash, PartialEq, Eq)]
#[display(fmt = "{}-{}", owner, id)]
#[sql_type = "Text"]
pub struct ProposalId {
    id: String,
    owner: Owner,
}

impl ProposalId {
    pub fn generate_id(
        offer_id: &SubscriptionId,
        demand_id: &SubscriptionId,
        creation_ts: &NaiveDateTime,
        owner: Owner,
    ) -> ProposalId {
        ProposalId {
            owner,
            id: hash_proposal(offer_id, demand_id, creation_ts),
        }
    }

    pub fn owner(&self) -> Owner {
        self.owner.clone()
    }

    pub fn translate(mut self, new_owner: Owner) -> Self {
        self.owner = new_owner;
        self
    }

    pub fn swap_owner(mut self) -> Self {
        self.owner = self.owner.swap();
        self
    }

    pub fn validate(
        &self,
        offer_id: &SubscriptionId,
        demand_id: &SubscriptionId,
        creation_ts: &NaiveDateTime,
    ) -> Result<(), ProposalIdValidationError> {
        let hash = hash_proposal(offer_id, demand_id, creation_ts);
        if self.id != hash {
            return Err(ProposalIdValidationError(self.clone(), hash));
        }
        Ok(())
    }

    /// Clients on both Requestor and Provider side should use the same id,
    /// because they communicate with each other and exchange this id.
    pub fn into_client(&self) -> String {
        self.id.clone()
    }

    pub fn from_client(s: &str, owner: Owner) -> Result<ProposalId, ProposalIdParseError> {
        if !s.chars().all(|character| character.is_ascii_hexdigit()) {
            Err(ProposalIdParseError::NotHexadecimal(s.to_string()))?;
        }

        if s.len() != HASH_SUFFIX_LEN {
            Err(ProposalIdParseError::InvalidLength(s.to_string()))?;
        }

        Ok(ProposalId {
            owner,
            id: s.to_string(),
        })
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

        let owner = Owner::from_str(elements[0])?;
        ProposalId::from_client(elements[1], owner)
    }
}

impl FromStr for Owner {
    type Err = ProposalIdParseError;

    fn from_str(s: &str) -> Result<Owner, Self::Err> {
        if s.len() != 1 {
            Err(ProposalIdParseError::InvalidOwner(s.to_string()))?;
        }

        Ok(match s.chars().next().unwrap() {
            'P' => Owner::Provider,
            'R' => Owner::Requestor,
            _ => Err(ProposalIdParseError::InvalidOwner(s.to_string()))?,
        })
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
