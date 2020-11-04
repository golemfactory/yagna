use actix_web::{web, Responder, Scope};
use futures::lock::Mutex;
use lazy_static::lazy_static;
use std::sync::Arc;

use ya_service_api_interfaces::Provider;

use crate::metrics::Metrics;

pub struct MetricsService;

lazy_static! {
    static ref METRICS: Arc<Mutex<Metrics>> = Metrics::new();
}

// TODO: enable showing metrics also via CLI
// impl Service for Metrics {
//     type Cli = ?;
// }

impl MetricsService {
    // currently just to produce log entry that service is activated
    pub async fn gsb<C: Provider<Self, ()>>(_ctx: &C) -> anyhow::Result<()> {
        // This should initialize Metrics. We need to do this before all other services will start.
        let _ = METRICS.clone();
        Ok(())
    }

    pub fn rest<C: Provider<Self, ()>>(_ctx: &C) -> actix_web::Scope {
        Scope::new("metrics-api/v1")
            // TODO:: add wrapper injecting Bearer to avoid hack in auth middleware
            .data(METRICS.clone())
            .service(expose_metrics)
    }
}

#[actix_web::get("/expose")]
pub async fn expose_metrics(metrics: web::Data<Arc<Mutex<Metrics>>>) -> impl Responder {
    metrics.lock().await.export()
}

pub async fn push(push_url: String) {
    let current_metrics = METRICS.clone().lock().await.export();
    let client = reqwest::Client::new();
    let res = client
        .put(push_url.as_str())
        .body(current_metrics)
        .send()
        .await;
    log::trace!("Pushed current metrics {:#?}", res);
}
