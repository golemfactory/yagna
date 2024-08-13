use diesel::backend::Backend;
use diesel::deserialize::{FromSql, FromSqlRow};
use diesel::serialize::{Output, ToSql};
use diesel::sql_types::{Text};
use diesel::sqlite::Sqlite;
use diesel::{deserialize, Queryable, serialize};
use std::io::Write;
use serde::Serialize;

pub trait AdaptDuration {
    fn adapt(self) -> DurationAdapter;
}

/// Sqlite Timestamp formatting omits sub-second parts if it is equal to zero.
/// This results in invalid comparison between `2022-07-29 12:33:14` and `2022-07-29 12:33:14.000`,
/// because sqlite compares text.
///
/// This adapter enforces timestamp format in database so it is suitable for comparison.
///
/// Check description of related issues:
/// https://github.com/golemfactory/yagna/issues/2145
/// https://github.com/golemfactory/yagna/pull/2086
#[derive(Clone, Debug, AsExpression)]
#[sql_type = "Text"]
pub struct DurationAdapter(pub chrono::Duration);

impl Serialize for DurationAdapter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        humantime::format_duration(self.0.to_std().unwrap_or_default()).to_string().serialize(serializer)
    }
}

impl<DB> FromSqlRow<Text, DB> for DurationAdapter
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn build_from_row<R: diesel::row::Row<DB>>(row: &mut R) -> deserialize::Result<Self> {
        let value = String::build_from_row(row)?;
        let chrono_duration = chrono::Duration::from_std( humantime::parse_duration(&value)?)?;
        Ok(DurationAdapter(chrono_duration))
    }
}


impl Queryable<Text, Sqlite> for DurationAdapter {
    type Row = DurationAdapter;

    fn build(row: Self::Row) -> Self {
        row
    }
}

impl<DB> FromSql<String, DB> for DurationAdapter
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {

        let value = String::from_sql(bytes)?;

        let chrono_duration = chrono::Duration::from_std( humantime::parse_duration(&value)?)?;

        Ok(chrono_duration.adapt())
    }
}

impl ToSql<Text, Sqlite> for DurationAdapter {
    fn to_sql<W: Write>(&self, out: &mut Output<W, Sqlite>) -> serialize::Result {
        let f = humantime::format_duration(self.0.to_std().unwrap_or_default()).to_string();
        ToSql::<Text, Sqlite>::to_sql(&f, out)
    }
}

impl AdaptDuration for chrono::Duration {
    fn adapt(self) -> DurationAdapter {
        DurationAdapter(self)
    }
}
