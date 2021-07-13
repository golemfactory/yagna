use anyhow::anyhow;
use metrics::counter;
use self_update::backends::github::UpdateBuilder;
use std::convert::TryFrom;

use ya_core_model::version::Release;
use ya_persistence::executor::DbExecutor;

use crate::db::dao::ReleaseDAO;
use crate::db::model::DBRelease;
use crate::service::cli::ReleaseMessage;

const REPO_OWNER: &'static str = "golemfactory";
const REPO_NAME: &'static str = "yagna";

pub async fn check_latest_release(db: &DbExecutor) -> anyhow::Result<Release> {
    log::debug!("Checking latest Yagna release");
    let gh_rel = tokio::task::spawn_blocking(|| -> anyhow::Result<self_update::update::Release> {
        Ok(UpdateBuilder::new()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name("") // seems required by builder but unused
            .current_version("") // similar as above
            .build()?
            .get_latest_release()?)
    })
    .await??;

    log::trace!("Got latest Yagna release {:?}", gh_rel);

    let db_rel = DBRelease::try_from(gh_rel)?;
    let rel = match db.as_dao::<ReleaseDAO>().save_new(db_rel.clone()).await {
        Err(e) => {
            let r = db_rel.into();
            log::error!("Storing new Yagna release {} to DB. {}", r, e);
            r
        }
        Ok(r) => r,
    };

    if self_update::version::bump_is_greater(ya_compile_time_utils::semver_str!(), &rel.version)
        .map_err(|e| {
            anyhow!(
                "Github release version `{}` parse error: {}",
                rel.version,
                e
            )
        })?
    {
        counter!("version.new", 1);
        log::warn!("{}", ReleaseMessage::Available(&rel));
    };
    Ok(rel)
}

pub(crate) async fn check_running_release(db: &DbExecutor) -> anyhow::Result<Release> {
    if let Some(release) = db.as_dao::<ReleaseDAO>().current_release().await? {
        return Ok(release);
    }

    let running_tag = ya_compile_time_utils::git_tag!();
    log::debug!("Checking release for running tag: {}", running_tag);

    let db_rel = match tokio::task::spawn_blocking(move || {
        UpdateBuilder::new()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name("") // seems required by builder but unused
            .current_version("") // similar as above
            .build()?
            .get_release_version(running_tag)
    })
    .await?
    {
        Ok(gh_rel) => {
            log::trace!("Got currently running release: {:?}", gh_rel);
            DBRelease::try_from(gh_rel)?
        }
        Err(e) => {
            log::trace!(
                "Failed to get release for running tag: '{}': {}. Using current",
                running_tag,
                e
            );
            DBRelease::current()?
        }
    };

    let rel = match db.as_dao::<ReleaseDAO>().save_new(db_rel.clone()).await {
        Err(e) => {
            let r = db_rel.into();
            log::error!("Storing running Yagna release {} to DB: {}", r, e);
            r
        }
        Ok(r) => {
            log::info!("Stored currently running Yagna release {} to DB", r);
            r
        }
    };
    Ok(rel)
}
