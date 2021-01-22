use anyhow::anyhow;
use metrics::counter;
use self_update::backends::github::UpdateBuilder;
use self_update::update::Release;
use self_update::version;
use std::time::Duration;

use ya_persistence::executor::DbExecutor;

use crate::db::dao::ReleaseDAO;
use crate::db::model::DBRelease;

pub(crate) const DEFAULT_RELEASE_TS: &'static str = "2015-10-13T15:43:00GMT+2";

#[derive(thiserror::Error, Debug, Clone)]
#[error("New Yagna release named '{}' (v{}) is available", .0.name, .0.version)]
pub(crate) enum ReleaseMessage<'a> {
    Available(&'a ya_core_model::version::Release),
    AvailableDB(&'a DBRelease),
    #[error("Release '{}' (v{}) skipped", .0.name, .0.version)]
    Skipped(&'a ya_core_model::version::Release),
}

pub async fn check_latest_release(db: &DbExecutor) -> anyhow::Result<()> {
    log::trace!("Checking latest Yagna release");
    let release = UpdateBuilder::new()
        .repo_owner("golemfactory")
        .repo_name("yagna")
        .bin_name("") // seems required by builder but unused
        .current_version("") // similar as above
        .build()?
        .get_latest_release()?;
    log::trace!(
        "Got latest Yagna release: '{}' (v{})",
        release.name,
        release.version
    );
    if version::bump_is_greater(
        ya_compile_time_utils::semver_str(),
        release.version.as_str(),
    )
    .map_err(|e| anyhow!("Github release `{:?}` parse error: {}", release, e))?
    {
        match db.as_dao::<ReleaseDAO>().new_release(&release).await {
            Err(e) => log::error!("Storing new Yagna release `{:?}` to DB. {}", release, e),
            Ok(r) => {
                counter!("version.new", 1);
                new_version_log(&r);
            }
        }
    };
    Ok(())
}

pub async fn on_start(db: &DbExecutor) -> anyhow::Result<()> {
    let worker_db = db.clone();
    tokio::task::spawn_local(async move { crate::notifier::worker(worker_db).await });
    let pinger_db = db.clone();
    tokio::task::spawn_local(async move { crate::notifier::pinger(pinger_db).await });

    let release_dao = db.as_dao::<ReleaseDAO>();
    release_dao
        .new_release(&Release {
            name: "".into(),
            version: ya_compile_time_utils::semver_str().into(),
            date: DEFAULT_RELEASE_TS.into(),
            body: None,
            assets: vec![],
        })
        .await
        .map_err(|e| anyhow::anyhow!("Storing current Yagna version as release to DB: {}", e))?;
    Ok(())
}

pub(crate) async fn worker(db: DbExecutor) {
    // TODO: make interval configurable
    let mut interval = tokio::time::interval(Duration::from_secs(3600 * 24));
    loop {
        interval.tick().await;
        if let Err(e) = check_latest_release(&db).await {
            log::error!("Failed to check for new Yagna release: {}", e);
        };
    }
}

pub(crate) async fn pinger(db: DbExecutor) -> ! {
    // TODO: make interval configurable
    // TODO: after test make it 30min instead 30sec
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        let release_dao = db.as_dao::<ReleaseDAO>();
        interval.tick().await;
        match release_dao.pending_release().await {
            Ok(Some(release)) => {
                if !release.seen {
                    new_version_log(&release)
                }
            }
            Ok(None) => log::trace!("Your Yagna is up to date"),
            Err(e) => log::error!("Fetching new Yagna release from DB: {}", e),
        }
    }
}

fn new_version_log(release: &DBRelease) {
    log::warn!("{}", ReleaseMessage::AvailableDB(release))
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use chrono::NaiveDateTime;

    #[test]
    fn test_default_release_ts() -> Result<()> {
        NaiveDateTime::parse_from_str(&crate::notifier::DEFAULT_RELEASE_TS, "%Y-%m-%dT%H:%M:%S%Z")?;
        Ok(())
    }

    /*

    #[tokio::test]
    async fn test_check_release() -> Result<()> {
        let result = crate::notifier::check_release().await?;
        println!("Check version result: {:#?}", result);
        println!("Current version: {}", ya_compile_time_utils::semver_str());
        assert!(false);
        Ok(())
    }
    */
}
