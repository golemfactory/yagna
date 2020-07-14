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

use crate::SubscriptionId;
use ya_client::model::ErrorMessage;

#[derive(Display, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
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

#[derive(
    Display, Debug, Clone, AsExpression, FromSqlRow, Hash, PartialEq, Eq, Serialize, Deserialize,
)]
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
