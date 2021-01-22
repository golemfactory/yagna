#[macro_use]
extern crate diesel;

pub(crate) mod db;
mod rest;

pub use rest::VersionService;

pub mod notifier {
    use std::time::Duration;
    use tokio::time;

    use ya_persistence::executor::DbExecutor;

    const UPDATE_CURL: &'static str = "curl -sSf https://join.golem.network/as-provider | bash -";
    const SILENCE_CMD: &'static str = "yagna update skip";
    const DEFAULT_RELEASE_TS: &'static str = "2015-10-13T15:43:00GMT+2";

    pub async fn check_release(
    ) -> Result<Vec<self_update::update::Release>, self_update::errors::Error> {
        log::trace!("Market release checker started");
        let releases = self_update::backends::github::ReleaseList::configure()
            .repo_owner("golemfactory")
            .repo_name("yagna")
            .build()?
            .fetch()?;
        log::trace!("Market release checker done");
        Ok(releases
            .into_iter()
            .filter(|r| {
                self_update::version::bump_is_greater(
                    ya_compile_time_utils::semver_str(),
                    r.version.as_str(),
                )
                .map_err(|e| log::warn!("Github version parse error. {}", e))
                .unwrap_or(false)
            })
            .collect())
    }

    pub async fn on_start(db: DbExecutor) -> anyhow::Result<()> {
        let release_dao = db.as_dao::<crate::db::dao::ReleaseDAO>();
        release_dao
            .new_release(self_update::update::Release {
                name: "".into(),
                version: ya_compile_time_utils::semver_str().into(),
                date: DEFAULT_RELEASE_TS.into(),
                body: None,
                assets: vec![],
            })
            .await?;
        Ok(())
    }
    pub async fn worker(db: DbExecutor) {
        let mut interval = time::interval(Duration::from_secs(3600 * 24));
        let release_dao = db.as_dao::<crate::db::dao::ReleaseDAO>();
        loop {
            interval.tick().await;
            match check_release().await {
                Err(e) => log::debug!(
                    "Problem encountered during checking for new releases: {}",
                    e
                ),
                Ok(releases) => {
                    for r in releases.into_iter() {
                        release_dao
                            .new_release(r)
                            .await
                            .map_err(|e| log::debug!("Problem storing new release. {}", e))
                            .ok();
                    }
                }
            };
        }
    }

    pub async fn pinger(db: DbExecutor) {
        let mut interval = time::interval(Duration::from_secs(60 * 24));
        let release_dao = db.as_dao::<crate::db::dao::ReleaseDAO>();
        loop {
            interval.tick().await;
            if let Ok(Some(db_release)) = release_dao.pending_release().await {
                log::warn!("New version of yagna available {}! Close yagna and run `{}` to install or `{}` to mute this notification", db_release, UPDATE_CURL, SILENCE_CMD);
            };
        }
    }
}

#[cfg(test)]
mod tests {
    /*
    use anyhow::Result;

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
