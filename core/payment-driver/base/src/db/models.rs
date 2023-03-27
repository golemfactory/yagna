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
    Unused = 0, //previous failure
    Created = 1,
    Sent = 2,
    Pending = 3,
    Confirmed = 4,
    Resend = 5,
    ResendAndBumpGas = 6,
    ErrorSent = 10,
    ErrorOnChain = 11,
    ErrorNonceTooLow = 12,
}

impl TryFrom<i32> for TransactionStatus {
    type Error = DbError;

    fn try_from(status: i32) -> DbResult<Self> {
        TransactionStatus::from_i32(status)
            .ok_or_else(|| DbError::InvalidData(format!("Unknown tx status. {}", status)))
    }
}

#[derive(Clone, Queryable, Debug, Identifiable, Insertable, PartialEq, Eq)]
#[primary_key(tx_id)]
#[table_name = "transaction"]
pub struct TransactionEntity {
    pub tx_id: String,
    pub sender: String,
    pub nonce: i32,
    pub status: i32,
    pub tx_type: i32,
    pub tmp_onchain_txs: Option<String>,
    pub final_tx: Option<String>,
    pub network: Network,
    pub starting_gas_price: Option<String>,
    pub current_gas_price: Option<String>,
    pub max_gas_price: Option<String>,
    pub final_gas_used: Option<i32>,
    pub amount_base: Option<String>,
    pub amount_erc20: Option<String>,
    pub gas_limit: Option<i32>,
    pub time_created: NaiveDateTime,
    pub time_last_action: NaiveDateTime,
    pub time_sent: Option<NaiveDateTime>,
    pub time_confirmed: Option<NaiveDateTime>,
    pub last_error_msg: Option<String>,
    pub resent_times: i32,
    pub signature: Option<String>,
    pub encoded: String,
}

#[derive(Queryable, Clone, Debug, Identifiable, Insertable, PartialEq, Eq)]
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

#[derive(AsExpression, FromSqlRow, PartialEq, Eq, Debug, Clone, Copy, FromPrimitive)]
#[sql_type = "Integer"]
pub enum Network {
    Mainnet = 1,    //Main Ethereum chain
    Rinkeby = 4,    //Rinkeby is Ethereum testnet
    Goerli = 5,     //Goerli is another Ethereum testnet
    Mumbai = 80001, //Mumbai is testnet for Polygon network
    Polygon = 137,  //Polygon is Polygon production network
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
            "goerli" => Ok(Network::Goerli),
            "polygon" => Ok(Network::Polygon),
            "mumbai" => Ok(Network::Mumbai),
            _ => Err(DbError::InvalidData(format!("Invalid network: {}", s))),
        }
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match *self {
            Network::Mainnet => f.write_str("mainnet"),
            Network::Rinkeby => f.write_str("rinkeby"),
            Network::Goerli => f.write_str("goerli"),
            Network::Mumbai => f.write_str("mumbai"),
            Network::Polygon => f.write_str("polygon"),
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
            5 => Network::Goerli,
            137 => Network::Polygon,
            80001 => Network::Mumbai,
            _ => return Err(anyhow::anyhow!("invalid value").into()),
        })
    }
}
