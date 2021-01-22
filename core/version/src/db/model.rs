use crate::db::schema::version_release;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

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

impl std::fmt::Display for DBRelease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} '{}' released @ {}",
            self.version, self.name, self.release_ts
        )
    }
}

impl From<DBRelease> for ya_core_model::version::Release {
    fn from(r: DBRelease) -> Self {
        Self {
            version: r.version,
            name: r.name,
            seen: r.seen,
            release_ts: r.release_ts,
            insertion_ts: r.insertion_ts,
            update_ts: r.update_ts,
        }
    }
}
