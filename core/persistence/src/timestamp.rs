use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::deserialize::FromSql;
use diesel::serialize::{IsNull, Output, ToSql};
use diesel::sql_types::Timestamp;
use diesel::sqlite::{Sqlite, SqliteValue};
use diesel::{deserialize, serialize};
use serde::Serialize;

pub trait AdaptTimestamp {
    fn adapt(self) -> TimestampAdapter;
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
#[derive(Clone, Debug, AsExpression, FromSqlRow, Serialize)]
#[diesel(sql_type = Timestamp)]
pub struct TimestampAdapter(pub NaiveDateTime);

impl FromSql<Timestamp, Sqlite> for TimestampAdapter {
    fn from_sql(value: SqliteValue<'_, '_, '_>) -> deserialize::Result<Self> {
        Ok(<NaiveDateTime as FromSql<Timestamp, Sqlite>>::from_sql(value)?.adapt())
    }
}

impl ToSql<Timestamp, Sqlite> for TimestampAdapter {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
        out.set_value(self.format());
        Ok(IsNull::No)
    }
}

impl TimestampAdapter {
    pub fn format(&self) -> String {
        self.0.format("%F %T.%6f").to_string()
    }
}

impl AdaptTimestamp for NaiveDateTime {
    fn adapt(self) -> TimestampAdapter {
        TimestampAdapter(self)
    }
}

impl AdaptTimestamp for DateTime<Utc> {
    fn adapt(self) -> TimestampAdapter {
        TimestampAdapter(self.naive_utc())
    }
}

#[cfg(test)]
mod tests {
    use crate::types::AdaptTimestamp;
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    use test_case::test_case;

    #[test_case(NaiveDateTime::new(
        NaiveDate::from_ymd_opt(2022, 7, 29).unwrap(),
        NaiveTime::from_hms_micro_opt(12, 33, 14, 0).unwrap(),
    ) => "2022-07-29 12:33:14.000000".to_string(); "0 microseconds should be always printed")]
    #[test_case(NaiveDateTime::new(
        NaiveDate::from_ymd_opt(2022, 7, 29).unwrap(),
        NaiveTime::from_hms_opt(12, 33, 14).unwrap(),
    ) => "2022-07-29 12:33:14.000000".to_string(); "0 microseconds should be always printed even if creating with from_hms")]
    #[test_case(NaiveDateTime::new(
        NaiveDate::from_ymd_opt(2022, 7, 29).unwrap(),
        NaiveTime::from_hms_micro_opt(12, 33, 14, 123456).unwrap(),
    ) => "2022-07-29 12:33:14.123456".to_string(); "non zero microseconds should be printed")]
    #[test_case(NaiveDateTime::new(
        NaiveDate::from_ymd_opt(2022, 7, 29).unwrap(),
        NaiveTime::from_hms_nano_opt(12, 33, 14, 123456789).unwrap(),
    ) => "2022-07-29 12:33:14.123456".to_string(); "nanoseconds should be truncated")]
    fn test_timestamp_adapter_formatting(timestamp: NaiveDateTime) -> String {
        timestamp.adapt().format()
    }
}
