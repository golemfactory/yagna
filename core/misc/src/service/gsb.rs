use metrics::counter;

use ya_core_model::misc;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{typed as bus, RpcMessage};
use ya_metrics::service::export_metrics_json;
use anyhow::anyhow;
use serde_json::Value;
use chrono::prelude::*;
use chrono::{NaiveDateTime, NaiveDate};

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

pub fn bind_gsb(db: &DbExecutor) {
    bus::ServiceBinder::new(misc::BUS_ID, db, ())
        .bind(get_misc_gsb);

    // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
    // until first change to value will be made.
    counter!("version.new", 0);
    counter!("version.skip", 0);
}



async fn get_misc_gsb(
    db: DbExecutor,
    _caller: String,
    msg: misc::MiscGet,
) -> RpcMessageResult<misc::MiscGet> {

    let metrics = export_metrics_json().await;

    let v: Value = serde_json::from_str(metrics.as_str()).map_err(|e| e.to_string())?;





   // let naive = );
    //let datetime: DateTime<Utc> = DateTime::from_utc(naive, Utc);

    Ok(misc::MiscInfo
    {
        test: "testing gsb misc".to_string(),
        is_net_connected: v["net.is_connected"].as_i64(),
        last_connected_time: v["net.last_connected_time"].as_i64(),
        last_disconnnected_time: v["net.last_disconnected_time"].as_i64(),
        metrics: metrics,
    })


}
