use crate::DRIVER_NAME;
use diesel::backend::Backend;
use diesel::deserialize;
use diesel::serialize::Output;
use diesel::sql_types::Integer;
use diesel::types::{FromSql, ToSql};
use maplit::hashmap;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;
use ya_client_model::payment::network as pay_model;

#[derive(AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy, FromPrimitive)]
#[sql_type = "Integer"]
pub enum Network {
    Mainnet = 1,
    Rinkeby = 4,
}

impl Network {
    pub fn chain_id(&self) -> u64 {
        *self as u64
    }

    pub fn default_token(&self) -> String {
        match *self {
            Network::Mainnet => "GLM".to_string(),
            Network::Rinkeby => "tGLM".to_string(),
        }
    }

    pub fn default_platform(&self) -> String {
        format!("{}-{}-{}", DRIVER_NAME, self, self.default_token()).to_lowercase()
    }
}

impl Default for Network {
    fn default() -> Self {
        Network::Rinkeby
    }
}

impl Into<pay_model::Network> for Network {
    fn into(self) -> pay_model::Network {
        let default_token = self.default_token();
        let default_platform = self.default_platform();
        let tokens = hashmap! {
            default_token.clone() => default_platform
        };
        pay_model::Network {
            default_token,
            tokens,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
#[error("Invalid network: {0}")]
pub struct InvalidNetworkError(pub String);

impl FromStr for Network {
    type Err = InvalidNetworkError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mainnet" => Ok(Network::Mainnet),
            "rinkeby" => Ok(Network::Rinkeby),
            _ => Err(InvalidNetworkError(s.to_string())),
        }
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match *self {
            Network::Mainnet => f.write_str("mainnet"),
            Network::Rinkeby => f.write_str("rinkeby"),
        }
    }
}

impl<DB: Backend> ToSql<Integer, DB> for Network
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<W: std::io::Write>(&self, out: &mut Output<W, DB>) -> diesel::serialize::Result {
        (*self as i32).to_sql(out)
    }
}

impl<DB> FromSql<Integer, DB> for Network
where
    i32: FromSql<Integer, DB>,
    DB: Backend,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        Ok(match i32::from_sql(bytes)? {
            1 => Network::Mainnet,
            4 => Network::Rinkeby,
            _ => return Err(anyhow::anyhow!("invalid value").into()),
        })
    }
}
