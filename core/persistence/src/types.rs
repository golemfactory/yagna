use bigdecimal::{BigDecimal, Zero};
use diesel::backend::Backend;
use diesel::deserialize::{FromSql, Result as DeserializeResult};
use diesel::serialize::{Output, Result as SerializeResult, ToSql};
use diesel::sql_types::Text;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::Write;
use std::ops::{Add, Sub};
use std::str::FromStr;

pub use crate::timestamp::{AdaptTimestamp, TimestampAdapter};

#[derive(Debug, Clone, AsExpression, FromSqlRow, Default, PartialEq, PartialOrd, Eq, Ord)]
#[sql_type = "Text"]
pub struct BigDecimalField(pub BigDecimal);

impl From<BigDecimalField> for BigDecimal {
    fn from(x: BigDecimalField) -> Self {
        x.0
    }
}

impl From<BigDecimal> for BigDecimalField {
    fn from(x: BigDecimal) -> Self {
        Self(x)
    }
}

impl Display for BigDecimalField {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}", self.0)
    }
}

impl Add<BigDecimalField> for BigDecimalField {
    type Output = BigDecimalField;

    fn add(self, rhs: BigDecimalField) -> Self::Output {
        (self.0 + rhs.0).into()
    }
}

impl<'a> Add<&'a BigDecimalField> for BigDecimalField {
    type Output = BigDecimalField;

    fn add(self, rhs: &'a BigDecimalField) -> Self::Output {
        (self.0 + &rhs.0).into()
    }
}

impl<'a, 'b> Add<&'b BigDecimalField> for &'a BigDecimalField {
    type Output = BigDecimalField;

    fn add(self, rhs: &'b BigDecimalField) -> Self::Output {
        (&self.0 + &rhs.0).into()
    }
}

impl Sub<BigDecimalField> for BigDecimalField {
    type Output = BigDecimalField;

    fn sub(self, rhs: BigDecimalField) -> Self::Output {
        (self.0 - rhs.0).into()
    }
}

impl<'a> Sub<&'a BigDecimalField> for BigDecimalField {
    type Output = BigDecimalField;

    fn sub(self, rhs: &'a BigDecimalField) -> Self::Output {
        (self.0 - &rhs.0).into()
    }
}

impl<'a, 'b> Sub<&'b BigDecimalField> for &'a BigDecimalField {
    type Output = BigDecimalField;

    fn sub(self, rhs: &'b BigDecimalField) -> Self::Output {
        (&self.0 - &rhs.0).into()
    }
}

impl<DB> ToSql<Text, DB> for BigDecimalField
where
    DB: Backend,
    String: ToSql<Text, DB>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> SerializeResult {
        let s = self.0.to_string();
        s.to_sql(out)
    }
}

impl<DB> FromSql<Text, DB> for BigDecimalField
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> DeserializeResult<Self> {
        let s = String::from_sql(bytes)?;
        match BigDecimal::from_str(&s) {
            Ok(x) => Ok(BigDecimalField(x)),
            Err(e) => Err(e.into()),
        }
    }
}

pub trait Summable {
    fn sum(self) -> BigDecimal;
}

impl<T> Summable for T
where
    T: IntoIterator,
    T::Item: Into<BigDecimal>,
{
    fn sum(self) -> BigDecimal {
        self.into_iter()
            .map(Into::into)
            .fold(BigDecimal::zero(), <BigDecimal as Add<BigDecimal>>::add)
    }
}

#[derive(Debug, Clone, Ord, Eq, PartialOrd, PartialEq, AsExpression, FromSqlRow)]
#[sql_type = "Text"]
pub enum Role {
    Provider,
    Requestor,
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid role string: {0}")]
pub struct RoleParseError(pub String);

impl ToString for Role {
    fn to_string(&self) -> String {
        match self {
            Role::Provider => "P".to_string(),
            Role::Requestor => "R".to_string(),
        }
    }
}

impl FromStr for Role {
    type Err = RoleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "P" => Ok(Role::Provider),
            "R" => Ok(Role::Requestor),
            _ => Err(RoleParseError(s.to_string())),
        }
    }
}

impl<DB> ToSql<Text, DB> for Role
where
    DB: Backend,
    String: ToSql<Text, DB>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> SerializeResult {
        let s = self.to_string();
        s.to_sql(out)
    }
}

impl<DB> FromSql<Text, DB> for Role
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> DeserializeResult<Self> {
        let s = String::from_sql(bytes)?;
        match Role::from_str(&s) {
            Ok(x) => Ok(x),
            Err(e) => Err(e.into()),
        }
    }
}
