use anyhow::anyhow;
use chrono::NaiveDateTime;
use diesel::dsl::{exists, select};
use diesel::prelude::*;
use self_update::update::Release;

use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

use crate::db::model::DBRelease;
use crate::db::schema::version_release::dsl as release;
use crate::db::schema::version_release::dsl::version_release;
use ya_core_model::version::VersionInfo;

pub struct ReleaseDAO<'c> {
    pool: &'c PoolType,
}
impl<'a> AsDao<'a> for ReleaseDAO<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> ReleaseDAO<'c> {
    pub async fn new_release(&self, r: &Release) -> anyhow::Result<DBRelease> {
        let db_release = DBRelease {
            version: r.version.clone(),
            name: r.name.clone(),
            seen: false,
            release_ts: NaiveDateTime::parse_from_str(&r.date, "%Y-%m-%dT%H:%M:%S%Z")?,
            insertion_ts: None,
            update_ts: None,
        };
        do_with_transaction(self.pool, move |conn| {
            if !select(exists(
                version_release.filter(release::version.eq(&db_release.version)),
            ))
            .get_result(conn)?
            {
                diesel::insert_into(version_release)
                    .values(&db_release)
                    .execute(conn)?;
            };
            Ok(db_release)
        })
        .await
    }

    pub async fn pending_release(&self) -> anyhow::Result<Option<DBRelease>> {
        readonly_transaction(self.pool, move |conn| get_pending_release(conn)).await
    }

    pub async fn version(&self) -> anyhow::Result<VersionInfo> {
        log::debug!("Getting Yagna version: current and pending from DB");
        readonly_transaction(self.pool, move |conn| {
            Ok(VersionInfo {
                current: get_current_release(conn)?
                    .ok_or(anyhow!("Can't determine current release."))?
                    .into(),
                pending: get_pending_release(conn)?.map(|r| r.into()),
            })
        })
        .await
    }

    pub async fn skip_pending_release(&self) -> anyhow::Result<Option<DBRelease>> {
        log::debug!("Skipping latest pending Yagna release");
        do_with_transaction(self.pool, move |conn| {
            let mut pending_release = match get_pending_release(conn)? {
                Some(r) => r,
                None => return Ok(None),
            };
            let num_updated = diesel::update(version_release.find(&pending_release.version))
                .set(release::seen.eq(true))
                .execute(conn)?;
            pending_release.seen = true;
            match num_updated {
                0 => anyhow::bail!("Release not skipped: {}", pending_release),
                1 => Ok(Some(pending_release)),
                _ => anyhow::bail!("More than one release skipped: {}", pending_release),
            }
        })
        .await
    }
}

fn get_current_release(conn: &ConnType) -> anyhow::Result<Option<DBRelease>> {
    Ok(version_release
        .filter(release::version.eq(ya_compile_time_utils::semver_str()))
        .first::<DBRelease>(conn)
        .optional()?)
}

fn get_pending_release(conn: &ConnType) -> anyhow::Result<Option<DBRelease>> {
    let query = version_release
        .filter(release::seen.eq(false))
        .order(release::release_ts.desc())
        .into_boxed();

    match query.first::<DBRelease>(conn).optional()? {
        Some(r) => {
            if !self_update::version::bump_is_greater(
                ya_compile_time_utils::semver_str(),
                r.version.as_str(),
            )
            .map_err(|e| log::warn!("Stored version parse error. {}", e))
            .unwrap_or(false)
            {
                return Ok(None);
            }
            Ok(Some(r))
        }
        None => Ok(None),
    }
}
