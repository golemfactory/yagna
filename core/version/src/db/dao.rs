use anyhow::anyhow;
use diesel::prelude::*;

use ya_core_model::version::{Release, VersionInfo};
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

use crate::db::model::DBRelease;
use crate::db::schema::version_release::dsl as release;
use crate::db::schema::version_release::dsl::version_release;
use self_update::version::bump_is_greater;

pub struct ReleaseDAO<'c> {
    pool: &'c PoolType,
}
impl<'a> AsDao<'a> for ReleaseDAO<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> ReleaseDAO<'c> {
    pub async fn save_new(&self, db_rel: DBRelease) -> anyhow::Result<Release> {
        do_with_transaction(self.pool, move |conn| {
            match get_release(conn, &db_rel.version)? {
                Some(rel) => Ok(rel),
                None => {
                    diesel::insert_into(version_release)
                        .values(&db_rel)
                        .execute(conn)?;
                    Ok(db_rel.into())
                }
            }
        })
        .await
    }

    pub async fn current_release(&self) -> anyhow::Result<Option<Release>> {
        readonly_transaction(self.pool, move |conn| get_current_release(conn)).await
    }

    pub async fn pending_release(&self) -> anyhow::Result<Option<Release>> {
        readonly_transaction(self.pool, move |conn| get_pending_release(conn, false)).await
    }

    pub async fn version(&self) -> anyhow::Result<VersionInfo> {
        log::debug!("Getting Yagna version: current and pending from DB");
        readonly_transaction(self.pool, move |conn| {
            Ok(VersionInfo {
                current: get_current_release(conn)?.ok_or(anyhow!("Can't get current release."))?,
                pending: get_pending_release(conn, true)?,
            })
        })
        .await
    }

    pub async fn skip_pending_release(&self) -> anyhow::Result<Option<Release>> {
        log::debug!("Skipping latest pending Yagna release");
        do_with_transaction(self.pool, move |conn| {
            let mut pending_rel = match get_pending_release(conn, false)? {
                Some(rel) => rel,
                None => return Ok(None),
            };
            let num_updated = diesel::update(version_release.find(&pending_rel.version))
                .set(release::seen.eq(true))
                .execute(conn)?;
            pending_rel.seen = true;
            match num_updated {
                0 => anyhow::bail!("Release not skipped: {}", pending_rel),
                1 => Ok(Some(pending_rel)),
                _ => anyhow::bail!("More than one release skipped: {}", pending_rel),
            }
        })
        .await
    }
}

fn get_current_release(conn: &ConnType) -> anyhow::Result<Option<Release>> {
    get_release(conn, ya_compile_time_utils::semver_str!())
}

fn get_release(conn: &ConnType, ver: &str) -> anyhow::Result<Option<Release>> {
    Ok(version_release
        .filter(release::version.eq(&ver))
        .first::<DBRelease>(conn)
        .optional()?
        .map(|db_rel| db_rel.into()))
}

fn get_pending_release(conn: &ConnType, include_seen: bool) -> anyhow::Result<Option<Release>> {
    let mut query = version_release
        // insertion_ts is to distinguish among fake-entries of `DBRelease::current`
        .filter(release::version.not_like("%rc%"))
        .order((release::release_ts.desc(), release::insertion_ts.desc()))
        .into_boxed();
    if !include_seen {
        query = query.filter(release::seen.eq(false));
    }

    match query.first::<DBRelease>(conn).optional()? {
        Some(db_rel) => {
            let running_ver = ya_compile_time_utils::semver_str!();
            if !bump_is_greater(running_ver, &db_rel.version)
                .map_err(|e| {
                    log::error!(
                        "Failed to compare if version {} > {}: {}",
                        running_ver,
                        db_rel.version,
                        e
                    )
                })
                .unwrap_or(false)
            {
                return Ok(None);
            }
            Ok(Some(db_rel.into()))
        }
        None => Ok(None),
    }
}
