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
use std::convert::TryFrom;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::str::FromStr;

// Local uses
use crate::dao::{DbError, DbResult};
use crate::db::schema::*;

pub const TX_CREATED: i32 = 1;
pub const TX_SENT: i32 = 2;
pub const TX_CONFIRMED: i32 = 3;
pub const TX_FAILED: i32 = 0;

pub const PAYMENT_STATUS_NOT_YET: i32 = 1;
pub const PAYMENT_STATUS_OK: i32 = 2;
pub const PAYMENT_STATUS_NOT_ENOUGH_FUNDS: i32 = 3;
pub const PAYMENT_STATUS_NOT_ENOUGH_GAS: i32 = 4;
pub const PAYMENT_STATUS_FAILED: i32 = 5;

pub enum TransactionStatus {
    Created,
    Sent,
    Confirmed,
    Failed,
}

impl TryFrom<i32> for TransactionStatus {
    type Error = DbError;

    fn try_from(status: i32) -> DbResult<Self> {
        match status {
            TX_CREATED => Ok(TransactionStatus::Created),
            TX_SENT => Ok(TransactionStatus::Sent),
            TX_CONFIRMED => Ok(TransactionStatus::Confirmed),
            TX_FAILED => Ok(TransactionStatus::Failed),
            _ => Err(DbError::InvalidData(format!(
                "Unknown tx status. {}",
                status
            ))),
        }
    }
}

impl Into<i32> for TransactionStatus {
    fn into(self) -> i32 {
        match &self {
            TransactionStatus::Created => TX_CREATED,
            TransactionStatus::Sent => TX_SENT,
            TransactionStatus::Confirmed => TX_CONFIRMED,
            TransactionStatus::Failed => TX_FAILED,
        }
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

#[derive(AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[sql_type = "Integer"]
pub enum Network {
    Mainnet = 1,
    Rinkeby = 4,
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
