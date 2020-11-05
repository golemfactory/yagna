use std::time::Duration;
use tokio::time;

use ya_core_model::identity;
use ya_service_bus::typed as bus;

pub fn spawn(host_url: url::Url) {
    log::debug!("Starting metrics pusher");
    tokio::task::spawn_local(async move {
        push_forever(host_url.as_str()).await;
    });
    log::info!("Metrics pusher started");
}

async fn get_default_id() -> anyhow::Result<String> {
    let default_id = bus::service(identity::BUS_ID)
        .call(identity::Get::ByDefault)
        .await??
        .ok_or(anyhow::anyhow!("Default identity not found"))?;
    Ok(default_id.node_id.to_string())
}

async fn try_get_default_id() -> anyhow::Result<String> {
    let mut interval = time::interval(Duration::from_secs(10));
    let mut last_error = None;
    for _ in 0..3 {
        interval.tick().await;
        match get_default_id().await {
            Ok(node_id) => return Ok(node_id),
            Err(e) => {
                log::debug!("Couldn't determine node_id. {:?}", e);
                last_error = Some(e);
            }
        }
    }
    Err(last_error.unwrap_or(anyhow::anyhow!("Undefined error")))
}

async fn get_push_url(host_url: &str, instance: &str) -> anyhow::Result<String> {
    let base = url::Url::parse(host_url)?;
    let url = base
        .join("/metrics/job/community.1/instance/")?
        .join(format!("{}/", instance).as_str())?
        .join("hostname/")?
        .join(instance)?;
    Ok(String::from(url.as_str()))
}

async fn push_forever(host_url: &str) {
    let node_id = match try_get_default_id().await {
        Ok(node_id) => node_id,
        Err(e) => {
            log::warn!("Couldn't determine node_id. Giving up. Err({})", e);
            return;
        }
    };
    let push_url = get_push_url(host_url, node_id.as_str()).await.unwrap();

    let mut interval = time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        push(push_url.clone()).await;
    }
}

pub async fn push(push_url: String) {
    let current_metrics = crate::service::expose_metrics().await;
    let client = reqwest::Client::new();
    let res = client
        .put(push_url.as_str())
        .body(current_metrics)
        .send()
        .await;
    log::trace!("Pushed current metrics {:#?}", res);
}
