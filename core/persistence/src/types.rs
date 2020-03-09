use bigdecimal::{BigDecimal, Zero};
use diesel::backend::Backend;
use diesel::deserialize::{FromSql, Result as DeserializeResult};
use diesel::serialize::{Output, Result as SerializeResult, ToSql};
use diesel::sql_types::Text;
use std::io::Write;
use std::ops::Add;
use std::str::FromStr;

#[derive(Debug, Clone, AsExpression, FromSqlRow, Default)]
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
