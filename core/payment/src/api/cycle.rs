use crate::dao::*;
use crate::utils::*;
use actix_web::web::{get, Data};
use actix_web::{web, HttpResponse, Scope};
use chrono::{Duration, NaiveDateTime};
use serde::Deserialize;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope.route("/batchCycle", get().to(get_batch_cycle))
}

async fn get_batch_cycle(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let dao: BatchCycleDao = db.as_dao();
    match dao.get_or_insert_default(node_id).await {
        Ok(batch_cycle) => response::ok(batch_cycle),
        Err(e) => response::server_error(&e),
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ProcessBatchCycleSetPost {
    interval_sec: Option<i64>,
    cron: Option<String>,
    extra_time_for_payment_sec: Option<i64>,
    next_update: Option<NaiveDateTime>,
}

async fn set_batch_cycle(
    db: Data<DbExecutor>,
    body: web::Json<ProcessBatchCycleSetPost>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let dao: BatchCycleDao = db.as_dao();
    let cycle_set = body.into_inner();
    let interval = cycle_set.interval_sec.map(Duration::seconds);
    let cron = cycle_set.cron;
    let extra_time_for_payment = cycle_set
        .extra_time_for_payment_sec
        .map(Duration::seconds)
        .unwrap_or(DEFAULT_EXTRA_TIME_FOR_PAYMENT);
    let next_update = cycle_set.next_update.map(|dt| dt.and_utc());

    match dao
        .create_or_update(
            node_id,
            interval,
            cron,
            Some(extra_time_for_payment),
            next_update,
        )
        .await
    {
        Ok(batch_cycle) => response::ok(batch_cycle),
        Err(e) => response::server_error(&e),
    }
}
