use std::time::Duration;

use ya_persistence::executor::DbExecutor;

use crate::db::dao::ReleaseDAO;
use crate::github;
use crate::github::check_running_release;
use crate::service::cli::ReleaseMessage;

pub async fn on_start(db: &DbExecutor) -> anyhow::Result<()> {
    check_running_release(&db).await?;

    if let Err(e) = github::check_latest_release(&db).await {
        log::error!("Failed to check for new Yagna release: {}", e);
    };

    let worker_db = db.clone();
    tokio::task::spawn_local(async move { crate::notifier::worker(worker_db).await });
    let pinger_db = db.clone();
    tokio::task::spawn_local(async move { crate::notifier::pinger(pinger_db).await });

    Ok(())
}

pub(crate) async fn worker(db: DbExecutor) {
    // TODO: make interval configurable
    let interval = Duration::from_secs(3600 * 24);
    loop {
        tokio::time::delay_for(interval).await;
        if let Err(e) = github::check_latest_release(&db).await {
            log::error!("Failed to check for new Yagna release: {}", e);
        };
    }
}

pub(crate) async fn pinger(db: DbExecutor) -> ! {
    // TODO: make interval configurable
    let interval = Duration::from_secs(30 * 60);
    loop {
        let release_dao = db.as_dao::<ReleaseDAO>();
        tokio::time::delay_for(interval).await;
        match release_dao.pending_release().await {
            Ok(Some(release)) => {
                if !release.seen {
                    log::warn!("{}", ReleaseMessage::Available(&release.into()))
                }
            }
            Ok(None) => log::trace!("Your Yagna is up to date"),
            Err(e) => log::error!("Fetching new Yagna release from DB: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDateTime;

    #[test]
    fn test_default_release_ts() {
        NaiveDateTime::parse_from_str(
            &format!("{}T00:00:00Z", ya_compile_time_utils::build_date()),
            "%Y-%m-%dT%H:%M:%S%Z",
        )
        .unwrap();
    }
}
