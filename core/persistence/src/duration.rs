use diesel::backend::Backend;
use diesel::deserialize::FromSql;
use diesel::serialize::{IsNull, Output, ToSql};
use diesel::sql_types::Text;
use diesel::sqlite::Sqlite;
use diesel::{deserialize, serialize};
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
#[derive(Clone, Debug, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
pub struct DurationAdapter(pub chrono::Duration);

impl Serialize for DurationAdapter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        humantime::format_duration(self.0.to_std().unwrap_or_default())
            .to_string()
            .serialize(serializer)
    }
}

impl<DB> FromSql<Text, DB> for DurationAdapter
where
    DB: Backend,
    String: FromSql<Text, DB>,
{
    fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
        let value = String::from_sql(bytes)?;

        let chrono_duration = chrono::Duration::from_std(humantime::parse_duration(&value)?)?;

        Ok(chrono_duration.adapt())
    }
}

impl ToSql<Text, Sqlite> for DurationAdapter {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
        out.set_value(humantime::format_duration(self.0.to_std().unwrap_or_default()).to_string());
        Ok(IsNull::No)
    }
}

impl AdaptDuration for chrono::Duration {
    fn adapt(self) -> DurationAdapter {
        DurationAdapter(self)
    }
}
