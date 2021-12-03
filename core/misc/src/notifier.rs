use std::time::Duration;
use metrics::gauge;
use chrono::Utc;

use ya_persistence::executor::DbExecutor;

pub async fn on_start(db: &DbExecutor) -> anyhow::Result<()> {

    let worker_db = db.clone();
    tokio::task::spawn_local(async move { crate::notifier::health_worker(worker_db).await });

    Ok(())
}

pub(crate) async fn health_worker(db: DbExecutor) {

    // TODO: make interval configurable
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        gauge!("health.last_loop_time", Utc::now().timestamp());

        log::info!("Performing health check");
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
