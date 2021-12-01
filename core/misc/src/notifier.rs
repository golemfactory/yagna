use std::time::Duration;

use ya_persistence::executor::DbExecutor;
use ya_metrics::service::export_metrics_json;

pub async fn on_start(db: &DbExecutor) -> anyhow::Result<()> {

    let worker_db = db.clone();
    tokio::task::spawn_local(async move { crate::notifier::worker(worker_db).await });
    let pinger_db = db.clone();
    tokio::task::spawn_local(async move { crate::notifier::pinger(pinger_db).await });

    Ok(())
}

pub(crate) async fn worker(db: DbExecutor) {

    // TODO: make interval configurable
    let mut interval = tokio::time::interval(Duration::from_secs(3600 * 24));
    loop {
        interval.tick().await;

        let metrics = export_metrics_json().await;
        log::info!("miscellaneous worker happily looping :)");
    }
}

pub(crate) async fn pinger(db: DbExecutor) -> ! {
    // TODO: make interval configurable

    let mut interval = tokio::time::interval(Duration::from_secs(30 * 60));
    loop {
        interval.tick().await;
        log::info!("miscellaneous pinger happily pinging :)");
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
