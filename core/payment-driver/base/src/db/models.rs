/*
    Raw database models.
*/

// External crates
use chrono::NaiveDateTime;
use diesel::backend::Backend;
use diesel::deserialize;
use diesel::serialize::Output;
use diesel::sql_types::Integer;
use diesel::types::{FromSql, ToSql};
use num_traits::FromPrimitive;
use std::convert::TryFrom;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;

// Local uses
use crate::dao::{DbError, DbResult};
use crate::db::schema::*;

pub const PAYMENT_STATUS_NOT_YET: i32 = 1;
pub const PAYMENT_STATUS_OK: i32 = 2;
pub const PAYMENT_STATUS_NOT_ENOUGH_FUNDS: i32 = 3;
pub const PAYMENT_STATUS_NOT_ENOUGH_GAS: i32 = 4;
pub const PAYMENT_STATUS_FAILED: i32 = 5;

#[derive(Clone, Copy)]
pub enum TxType {
    Faucet = 0,
    Transfer = 1,
}

#[derive(FromPrimitive)]
pub enum TransactionStatus {
    Failed = 0,
    Created = 1,
    Sent = 2,
    Confirmed = 3,
}

impl TryFrom<i32> for TransactionStatus {
    type Error = DbError;

    fn try_from(status: i32) -> DbResult<Self> {
        TransactionStatus::from_i32(status)
            .ok_or_else(|| DbError::InvalidData(format!("Unknown tx status. {}", status)))
    }
}

#[derive(Clone, Queryable, Debug, Identifiable, Insertable, PartialEq)]
#[primary_key(tx_hash)]
#[table_name = "transaction"]
pub struct TransactionEntity {
    pub tx_id: String,
    pub sender: String,
    pub nonce: String,
    pub timestamp: NaiveDateTime,
    pub status: i32,
    pub tx_type: i32,
    pub encoded: String,
    pub signature: String,
    pub tx_hash: Option<String>,
    pub network: Network,
}

#[derive(Queryable, Clone, Debug, Identifiable, Insertable, PartialEq)]
#[primary_key(order_id)]
#[table_name = "payment"]
pub struct PaymentEntity {
    pub order_id: String,
    pub amount: String,
    pub gas: String,
    pub sender: String,
    pub recipient: String,
    pub payment_due_date: NaiveDateTime,
    pub status: i32,
    pub tx_id: Option<String>,
    pub network: Network,
}

#[derive(AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy, FromPrimitive)]
#[sql_type = "Integer"]
pub enum Network {
    Mainnet = 1,
    Rinkeby = 4,
    PolygonMumbai = 80001,
    PolygonMainnet = 137
}

impl Default for Network {
    fn default() -> Self {
        Network::Rinkeby
    }
}

impl FromStr for Network {
    type Err = DbError;

    fn from_str(s: &str) -> DbResult<Self> {
        match s.to_lowercase().as_str() {
            "mainnet" => Ok(Network::Mainnet),
            "rinkeby" => Ok(Network::Rinkeby),
            "polygon" => Ok(Network::PolygonMainnet),
            "mumbai" => Ok(Network::PolygonMumbai),
            _ => Err(DbError::InvalidData(format!(
                "Invalid network: {}",
                s.to_string()
            ))),
        }
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match *self {
            Network::Mainnet => f.write_str("mainnet"),
            Network::Rinkeby => f.write_str("rinkeby"),
            Network::PolygonMumbai => f.write_str("mumbai"),
            Network::PolygonMainnet => f.write_str("polygon"),
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
            137 => Network::PolygonMainnet,
            80001 => Network::PolygonMumbai,
            _ => return Err(anyhow::anyhow!("invalid value").into()),
        })
    }
}
