use chrono::{DateTime, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

use crate::cdn::CdnRelease;
use crate::db::schema::version_release;

#[derive(Clone, Debug, Identifiable, Insertable, Queryable, Serialize, Deserialize)]
#[diesel(primary_key(version))]
#[diesel(table_name = version_release)]
pub struct DBRelease {
    pub version: String,
    pub name: String,
    pub seen: bool,
    pub release_ts: NaiveDateTime,
    pub insertion_ts: Option<NaiveDateTime>,
    pub update_ts: Option<NaiveDateTime>,
}

impl DBRelease {
    pub(crate) fn current() -> anyhow::Result<Self> {
        Ok(DBRelease {
            version: ya_compile_time_utils::semver_str!().into(),
            name: format!(
                "({} {}{})",
                ya_compile_time_utils::git_rev(),
                ya_compile_time_utils::build_date(),
                ya_compile_time_utils::build_number_str()
                    .map(|bn| format!(" build #{}", bn))
                    .unwrap_or_else(|| "".into())
            ),
            seen: true,
            release_ts: parse_release_ts(&format!(
                "{}T00:00:00Z",
                ya_compile_time_utils::build_date()
            ))?,
            insertion_ts: None,
            update_ts: None,
        })
    }
}

impl From<DBRelease> for ya_core_model::version::Release {
    fn from(db_rel: DBRelease) -> Self {
        Self {
            version: db_rel.version,
            name: db_rel.name,
            seen: db_rel.seen,
            release_ts: db_rel.release_ts,
            insertion_ts: db_rel.insertion_ts,
            update_ts: db_rel.update_ts,
        }
    }
}

impl TryFrom<CdnRelease> for DBRelease {
    type Error = anyhow::Error;
    fn try_from(release: CdnRelease) -> Result<Self, Self::Error> {
        Ok(Self {
            version: release.version,
            name: release.name,
            seen: false,
            release_ts: release.released_at.naive_utc(),
            insertion_ts: None,
            update_ts: None,
        })
    }
}

fn parse_release_ts(ts: &str) -> anyhow::Result<NaiveDateTime> {
    Ok(DateTime::parse_from_rfc3339(ts)?.naive_utc())
}

#[cfg(test)]
mod test {
    use crate::db::model::DBRelease;

    #[test]
    fn test_current() {
        let c = DBRelease::current().unwrap();
        assert_eq!(
            c.release_ts.format("%Y-%m-%d").to_string(),
            ya_compile_time_utils::build_date()
        );
    }
}
