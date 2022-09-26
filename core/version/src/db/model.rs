use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

use crate::db::schema::version_release;
use ya_compile_time_utils::tag2semver;

pub(crate) const DEFAULT_RELEASE_TS: &'static str = "2015-10-13T15:43:00GMT+2";

#[derive(Clone, Debug, Identifiable, Insertable, Queryable, Serialize, Deserialize)]
#[primary_key(version)]
#[table_name = "version_release"]
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
            release_ts: parse_release_ts(DEFAULT_RELEASE_TS)?,
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

impl TryFrom<self_update::update::Release> for DBRelease {
    type Error = anyhow::Error;
    fn try_from(rel: self_update::update::Release) -> Result<Self, Self::Error> {
        Ok(Self {
            version: tag2semver(&rel.version).into(),
            name: rel.name.clone(),
            seen: false,
            release_ts: parse_release_ts(&rel.date)?,
            insertion_ts: None,
            update_ts: None,
        })
    }
}

fn parse_release_ts(ts: &str) -> anyhow::Result<NaiveDateTime> {
    Ok(NaiveDateTime::parse_from_str(&ts, "%Y-%m-%dT%H:%M:%S%Z")?)
}

#[cfg(test)]
mod test {
    use crate::db::model::DBRelease;

    #[test]
    fn test_current() {
        let c = DBRelease::current().unwrap();
        println!("{:?}", c)
    }
}
