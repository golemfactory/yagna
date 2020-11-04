use futures::lock::Mutex;
use metrics_runtime::{observers::PrometheusBuilder, Controller, Receiver, Sink};
use std::sync::Arc;
use std::time::Duration;
use tokio::time;

use ya_core_model::identity;
use ya_service_bus::typed as bus;

use crate::exporter::StringExporter;

pub struct Metrics {
    //pub receiver: Receiver,
    pub root_sink: Sink,
    pub exporter: StringExporter<Controller, PrometheusBuilder>,
}

impl Metrics {
    pub fn new() -> Arc<Mutex<Metrics>> {
        let receiver = Receiver::builder()
            .build()
            .expect("Metrics initialization failure");
        let root_sink = receiver.sink();
        let exporter = StringExporter::new(receiver.controller(), PrometheusBuilder::new());
        receiver.install();

        log::debug!("Starting pusher");
        tokio::task::spawn_local(async move {
            push_forever().await;
        });
        log::info!("Started pusher");

        Arc::new(Mutex::new(Self {
            //receiver,
            root_sink,
            exporter,
        }))
    }

    #[allow(dead_code)]
    pub fn create_sink(&mut self, name: &str) -> std::sync::Mutex<Sink> {
        std::sync::Mutex::new(self.root_sink.scoped(name))
    }

    pub fn export(&mut self) -> String {
        return self.exporter.turn();
    }
}

const PUSH_HOST_URL: &str = "http://127.0.0.1:9091/";

async fn get_default_id() -> anyhow::Result<String> {
    let default_id = bus::service(identity::BUS_ID)
        .call(identity::Get::ByDefault)
        .await??
        .ok_or(anyhow::anyhow!("Default identity not found"))?;
    Ok(format!("{}", default_id.node_id))
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
        .join(instance)?;
    Ok(String::from(url.as_str()))
}

async fn push_forever() {
    let node_id = match try_get_default_id().await {
        Ok(node_id) => node_id,
        Err(e) => {
            log::warn!("Couldn't determine node_id. Giving up. Err({})", e);
            return;
        }
    };
    let push_url = get_push_url(PUSH_HOST_URL, node_id.as_str()).await.unwrap();

    let mut interval = time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        crate::service::push(push_url.clone()).await;
    }
}
