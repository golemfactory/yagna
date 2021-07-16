use awc::error::{ConnectError, SendRequestError};
use awc::Client;
use tokio::time::{self, Duration, Instant};

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use ya_core_model::identity::{self, IdentityInfo};
use ya_service_api::MetricsCtx;
use ya_service_bus::typed as bus;

pub fn spawn(ctx: MetricsCtx) {
    if !ctx.push_enabled {
        log::info!("Metrics pusher disabled");
        return;
    }

    log::debug!("Starting metrics pusher");
    tokio::task::spawn_local(async move {
        push_forever(ctx.push_host_url.unwrap().as_str()).await;
    });
    log::info!("Metrics pusher started");
}

pub async fn push_forever(host_url: &str) {
    let node_identity = match try_get_default_id().await {
        Ok(default_id) => default_id,
        Err(e) => {
            log::warn!(
                "Metrics pusher init failure: Couldn't determine node_id: {}",
                e
            );
            return;
        }
    };
    let push_url = match get_push_url(host_url, &node_identity) {
        Ok(url) => url,
        Err(e) => {
            log::warn!(
                "Metrics pusher init failure: Parsing URL: {} with {:?}: {}",
                host_url,
                node_identity,
                e
            );
            return;
        }
    };

    let start = Instant::now() + Duration::from_secs(5);
    let mut push_interval = time::interval_at(start, Duration::from_secs(60));
    let client = Client::builder().timeout(Duration::from_secs(30)).finish();
    log::info!("Starting metrics pusher");
    loop {
        push_interval.tick().await;
        push(&client, push_url.clone()).await;
    }
}

pub async fn push(client: &Client, push_url: String) {
    let metrics = crate::service::export_metrics().await;
    let res = client
        .put(push_url.as_str())
        .send_body(metrics.clone())
        .await;
    match res {
        Ok(r) if r.status().is_success() => {
            log::trace!("Metrics pushed: {}", r.status())
        }
        Ok(r) if r.status().is_server_error() => {
            log::debug!("Metrics server error: {:#?}", r);
            log::trace!("Url: {}\nMetrics:\n{}", push_url, metrics);
        }
        Ok(mut r) => {
            let body = r.body().await.unwrap_or_default().to_vec();
            let msg = String::from_utf8_lossy(&body);
            log::warn!("Pushing metrics failed: `{}`: {:#?}", msg, r);
            log::debug!("Url: {}", push_url);
            log::trace!("Metrics:\n{}", metrics);
        }
        Err(SendRequestError::Timeout) | Err(SendRequestError::Connect(ConnectError::Timeout)) => {
            log::debug!("Pushing metrics timed out");
            log::trace!("Url: {}\nMetrics:\n{}", push_url, metrics);
        }
        Err(e) => {
            log::info!("Pushing metrics failed: {}", e);
            log::debug!("Url: {}", push_url);
            log::trace!("Metrics:\n{}", metrics);
        }
    };
}

async fn get_default_id() -> anyhow::Result<IdentityInfo> {
    let default_id = bus::service(identity::BUS_ID)
        .call(identity::Get::ByDefault)
        .await??
        .ok_or(anyhow::anyhow!("Default identity not found"))?;
    Ok(default_id)
}

async fn try_get_default_id() -> anyhow::Result<IdentityInfo> {
    let mut interval = time::interval(Duration::from_secs(10));
    let mut last_error = None;
    for _ in 0..3 {
        interval.tick().await;
        match get_default_id().await {
            Ok(default_id) => return Ok(default_id),
            Err(e) => {
                log::debug!("Couldn't determine node_id. {:?}", e);
                last_error = Some(e);
            }
        }
    }
    Err(last_error.unwrap_or(anyhow::anyhow!("Undefined error")))
}

fn get_push_url(host_url: &str, id: &IdentityInfo) -> anyhow::Result<String> {
    let base = url::Url::parse(host_url)?;
    let url = base
        .join("/metrics/job/community.1/")?
        .join(&format!("instance/{}/", &id.node_id))?
        .join(&format!(
            "hostname/{}",
            id.alias
                .as_ref()
                .map(|alias| utf8_percent_encode(alias, NON_ALPHANUMERIC).to_string())
                .unwrap_or(id.node_id.to_string())
        ))?;
    Ok(String::from(url.as_str()))
}

#[cfg(test)]
mod test {
    use crate::pusher::get_push_url;
    use ya_core_model::identity::IdentityInfo;

    #[test]
    fn test_get_push_url_with_slashes() {
        let url = get_push_url(
            "http://a",
            &IdentityInfo {
                alias: Some("ala/ma/kota".into()),
                node_id: Default::default(),
                is_locked: false,
                is_default: false,
            },
        )
        .unwrap();
        assert_eq!("http://a/metrics/job/community.1/instance/0x0000000000000000000000000000000000000000/hostname/ala%2Fma%2Fkota", url);
    }

    #[test]
    fn test_get_push_url_with_pletters() {
        let url = get_push_url(
            "http://a",
            &IdentityInfo {
                alias: Some("zażółć?gęślą!jaźń=".into()),
                node_id: Default::default(),
                is_locked: false,
                is_default: false,
            },
        )
        .unwrap();
        assert_eq!("http://a/metrics/job/community.1/instance/0x0000000000000000000000000000000000000000/hostname/za%C5%BC%C3%B3%C5%82%C4%87%3Fg%C4%99%C5%9Bl%C4%85%21ja%C5%BA%C5%84%3D", url);
    }
}
