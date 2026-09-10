pub use crate::duration::{AdaptDuration, DurationAdapter};
pub use crate::timestamp::{AdaptTimestamp, TimestampAdapter};
use bigdecimal::{BigDecimal, Zero};
use diesel::backend::Backend;
use diesel::deserialize::{FromSql, Result as DeserializeResult};
use diesel::expression::AsExpression;
use diesel::serialize::{Output, Result as SerializeResult, ToSql};
use diesel::sql_types::Text;
use serde::Serialize;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::ops::{Add, Sub};
use std::str::FromStr;

macro_rules! impl_text_as_expression {
    ($type:ty) => {
        impl AsExpression<Text> for $type {
            type Expression = <String as AsExpression<Text>>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as AsExpression<Text>>::as_expression(self.to_string())
            }
        }

        impl AsExpression<diesel::sql_types::Nullable<Text>> for $type {
            type Expression =
                <String as AsExpression<diesel::sql_types::Nullable<Text>>>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as AsExpression<diesel::sql_types::Nullable<Text>>>::as_expression(
                    self.to_string(),
                )
            }
        }

        impl AsExpression<Text> for &$type {
            type Expression = <String as AsExpression<Text>>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as AsExpression<Text>>::as_expression(self.to_string())
            }
        }

        impl AsExpression<diesel::sql_types::Nullable<Text>> for &$type {
            type Expression =
                <String as AsExpression<diesel::sql_types::Nullable<Text>>>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as AsExpression<diesel::sql_types::Nullable<Text>>>::as_expression(
                    self.to_string(),
                )
            }
        }

        impl AsExpression<Text> for &&$type {
            type Expression = <String as AsExpression<Text>>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as AsExpression<Text>>::as_expression(self.to_string())
            }
        }

        impl AsExpression<diesel::sql_types::Nullable<Text>> for &&$type {
            type Expression =
                <String as AsExpression<diesel::sql_types::Nullable<Text>>>::Expression;

            fn as_expression(self) -> Self::Expression {
                <String as AsExpression<diesel::sql_types::Nullable<Text>>>::as_expression(
                    self.to_string(),
                )
            }
        }
    };
}

#[derive(Debug, Clone, FromSqlRow, Default, PartialEq, PartialOrd, Eq, Ord)]
#[diesel(sql_type = Text)]
pub struct BigDecimalField(pub BigDecimal);

impl_text_as_expression!(BigDecimalField);

impl Serialize for BigDecimalField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.to_plain_string().serialize(serializer)
    }
}

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
        // BigDecimal 0.4 may use scientific notation for small values. Amounts
        // are persisted as TEXT, so retain the plain format used by 0.2.
        f.write_str(&self.0.to_plain_string())
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

impl<'b> Add<&'b BigDecimalField> for &BigDecimalField {
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

impl<'b> Sub<&'b BigDecimalField> for &BigDecimalField {
    type Output = BigDecimalField;

    fn sub(self, rhs: &'b BigDecimalField) -> Self::Output {
        (&self.0 - &rhs.0).into()
    }
}

impl<DB> FromSql<Text, DB> for BigDecimalField
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: DB::RawValue<'_>) -> DeserializeResult<Self> {
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
#[diesel(sql_type = Text)]
pub enum Role {
    Provider,
    Requestor,
}
impl Serialize for Role {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid role string: {0}")]
pub struct RoleParseError(pub String);

impl Display for Role {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(self.as_str())
    }
}

impl Role {
    fn as_str(&self) -> &'static str {
        match self {
            Role::Provider => "P",
            Role::Requestor => "R",
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
    str: ToSql<Text, DB>,
{
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, DB>) -> SerializeResult {
        <str as ToSql<Text, DB>>::to_sql(self.as_str(), out)
    }
}

impl<DB> FromSql<Text, DB> for Role
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: DB::RawValue<'_>) -> DeserializeResult<Self> {
        let s = String::from_sql(bytes)?;
        match Role::from_str(&s) {
            Ok(x) => Ok(x),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_decimal_field_formats_small_values_plainly() {
        let field = BigDecimalField(BigDecimal::from_str("0.00000005").unwrap());

        assert_eq!(field.to_string(), "0.00000005");
        assert_eq!(serde_json::to_string(&field).unwrap(), "\"0.00000005\"");
    }

    #[test]
    fn big_decimal_field_formats_regular_values_plainly() {
        for value in ["0", "1", "123.45678", "1000000000000000000", "0.1"] {
            let field = BigDecimalField(BigDecimal::from_str(value).unwrap());

            assert_eq!(field.to_string(), value);
            assert_eq!(
                serde_json::to_string(&field).unwrap(),
                format!("\"{value}\"")
            );
        }
    }
}
