use crate::dao::*;
use crate::utils::*;
use actix_web::web::{get, post};
use actix_web::{web, HttpResponse, Scope};
use chrono::NaiveDateTime;
use serde::Deserialize;
use ya_core_model::payment::local as pay_local;
use ya_core_model::payment::local::{ProcessBatchCycleInfo, ProcessBatchCycleSet};
use ya_service_api_web::middleware::Identity;
use ya_service_bus::typed as bus;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/batchCycle/{platform}", get().to(get_batch_cycle))
        .route("/batchCycle", post().to(set_batch_cycle))
}

async fn get_batch_cycle(id: Identity, platform: web::Path<String>) -> HttpResponse {
    let node_id = id.identity;

    match bus::service(pay_local::BUS_ID)
        .call(ProcessBatchCycleInfo {
            node_id,
            platform: platform.to_string(),
        })
        .await
    {
        Ok(batch_cycle) => response::ok(batch_cycle),
        Err(e) => response::server_error(&e),
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ProcessBatchCycleSetPost {
    platform: String,
    interval_sec: Option<u64>,
    cron: Option<String>,
    extra_time_for_payment_sec: Option<u64>,
    next_update: Option<NaiveDateTime>,
}

async fn set_batch_cycle(body: web::Json<ProcessBatchCycleSetPost>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let cycle_set = body.into_inner();
    let interval: Option<core::time::Duration> =
        cycle_set.interval_sec.map(core::time::Duration::from_secs);
    let cron = cycle_set.cron;
    let extra_time_for_payment = cycle_set
        .extra_time_for_payment_sec
        .map(core::time::Duration::from_secs)
        .unwrap_or(DEFAULT_EXTRA_TIME_FOR_PAYMENT.to_std().unwrap());
    let next_update = cycle_set.next_update.map(|dt| dt.and_utc());

    match bus::service(pay_local::BUS_ID)
        .call(ProcessBatchCycleSet {
            node_id,
            platform: cycle_set.platform,
            interval,
            cron,
            next_update,
            safe_payout: Some(extra_time_for_payment),
        })
        .await
    {
        Ok(batch_cycle) => response::ok(batch_cycle),
        Err(e) => response::server_error(&e),
    }
}
