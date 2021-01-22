#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;

pub(crate) mod db;
mod rest;
pub mod service;

pub mod notifier {
    use metrics::counter;
    use std::time::Duration;

    use ya_persistence::executor::DbExecutor;

    use crate::db::dao::ReleaseDAO;

    pub(crate) const DEFAULT_RELEASE_TS: &'static str = "2015-10-13T15:43:00GMT+2";

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

    pub async fn on_start(db: &DbExecutor) -> anyhow::Result<()> {
        let worker_db = db.clone();
        tokio::task::spawn_local(async move { crate::notifier::worker(worker_db).await });
        let pinger_db = db.clone();
        tokio::task::spawn_local(async move { crate::notifier::pinger(pinger_db).await });

        let release_dao = db.as_dao::<ReleaseDAO>();
        release_dao
            .new_release(self_update::update::Release {
                name: "".into(),
                version: ya_compile_time_utils::semver_str().into(),
                date: DEFAULT_RELEASE_TS.into(),
                body: None,
                assets: vec![],
            })
            .await
            .map_err(|e| {
                anyhow::anyhow!("Storing current Yagna version as Release to DB: {}", e)
            })?;
        Ok(())
    }

    pub(crate) async fn worker(db: DbExecutor) {
        // TODO: make interval configurable
        let mut interval = tokio::time::interval(Duration::from_secs(3600 * 24));
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
                        counter!("version.new", 1);
                        release_dao
                            .new_release(r)
                            .await
                            .map_err(|e| log::error!("Storing new Yagna release to DB. {}", e))
                            .ok();
                    }
                }
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
                        log::warn!(
                            "New Yagna version {} ({}) is available. TODO: add howto here",
                            release.name,
                            release.version
                        )
                    }
                }
                Ok(None) => log::trace!("Your Yagna is up to date"),
                Err(e) => log::error!("Fetching new Yagna release from DB: {}", e),
            }
        }
    }
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
