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
