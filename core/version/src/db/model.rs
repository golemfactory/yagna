use chrono::{NaiveDateTime, Utc};
use self_update::update::Release;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

use crate::db::schema::version_release;

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
            version: ya_compile_time_utils::semver_str().into(),
            name: ya_compile_time_utils::version_describe!().into(),
            seen: true,
            release_ts: Utc::now().naive_utc(),
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
            gitrev: None,
            seen: db_rel.seen,
            release_ts: db_rel.release_ts,
            insertion_ts: db_rel.insertion_ts,
            update_ts: db_rel.update_ts,
        }
    }
}

impl TryFrom<self_update::update::Release> for DBRelease {
    type Error = anyhow::Error;
    fn try_from(rel: Release) -> Result<Self, Self::Error> {
        Ok(Self {
            version: rel.version.clone(),
            name: rel.name.clone(),
            seen: false,
            release_ts: NaiveDateTime::parse_from_str(&rel.date, "%Y-%m-%dT%H:%M:%S%Z")?,
            insertion_ts: None,
            update_ts: None,
        })
    }
}
