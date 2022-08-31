use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::backend::Backend;
use diesel::deserialize::FromSql;
use diesel::serialize::{Output, ToSql};
use diesel::sql_types::{Text, Timestamp};
use diesel::sqlite::Sqlite;
use diesel::{deserialize, serialize};
use std::io::Write;

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
#[derive(Clone, Debug, AsExpression)]
#[sql_type = "Timestamp"]
pub struct TimestampAdapter(pub NaiveDateTime);

impl FromSql<Timestamp, Sqlite> for TimestampAdapter {
    fn from_sql(value: Option<&<Sqlite as Backend>::RawValue>) -> deserialize::Result<Self> {
        Ok(NaiveDateTime::from_sql(value)?.adapt())
    }
}

impl ToSql<Timestamp, Sqlite> for TimestampAdapter {
    fn to_sql<W: Write>(&self, out: &mut Output<W, Sqlite>) -> serialize::Result {
        ToSql::<Text, Sqlite>::to_sql(&self.format(), out)
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
        NaiveDate::from_ymd(2022, 07, 29),
        NaiveTime::from_hms_micro(12, 33, 14, 0),
    ) => "2022-07-29 12:33:14.000000".to_string(); "0 microseconds should be always printed")]
    #[test_case(NaiveDateTime::new(
        NaiveDate::from_ymd(2022, 07, 29),
        NaiveTime::from_hms(12, 33, 14),
    ) => "2022-07-29 12:33:14.000000".to_string(); "0 microseconds should be always printed even if creating with from_hms")]
    #[test_case(NaiveDateTime::new(
        NaiveDate::from_ymd(2022, 07, 29),
        NaiveTime::from_hms_micro(12, 33, 14, 123456),
    ) => "2022-07-29 12:33:14.123456".to_string(); "non zero microseconds should be printed")]
    #[test_case(NaiveDateTime::new(
        NaiveDate::from_ymd(2022, 07, 29),
        NaiveTime::from_hms_nano(12, 33, 14, 123456789),
    ) => "2022-07-29 12:33:14.123456".to_string(); "nanoseconds should be truncated")]
    fn test_timestamp_adapter_formatting(timestamp: NaiveDateTime) -> String {
        timestamp.adapt().format()
    }
}
