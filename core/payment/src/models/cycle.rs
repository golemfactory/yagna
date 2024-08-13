use std::str::FromStr;
use crate::schema::*;
use anyhow::anyhow;
use chrono::{Duration, Utc};
use ya_client_model::NodeId;
use ya_persistence::types::{AdaptDuration, AdaptTimestamp, DurationAdapter, TimestampAdapter};

#[derive(Debug, Clone, Queryable, Insertable, AsChangeset)]
#[table_name = "pay_batch_cycle"]
pub struct DbPayBatchCycle {
    pub owner_id: String,
    pub created_ts: TimestampAdapter,
    pub updated_ts: TimestampAdapter,
    pub cycle_interval: Option<DurationAdapter>,
    pub cycle_cron: Option<String>,
    pub cycle_last_process: Option<TimestampAdapter>,
    pub cycle_next_process: TimestampAdapter,
    pub cycle_max_interval: DurationAdapter,
    pub cycle_max_pay_time: DurationAdapter,
}

pub fn createBatchCycleBasedOnInterval(
    owner_id: String,
    interval: Duration,
    extra_time_for_payment: Duration,
) -> anyhow::Result<DbPayBatchCycle> {
    if interval < Duration::seconds(5) {
        return Err(anyhow::anyhow!(
            "Interval must be greater than 5 seconds (at least 5 minutes suggested)"
        ));
    }
    if extra_time_for_payment < Duration::seconds(5) {
        return Err(anyhow::anyhow!(
            "Extra time for payment must be greater than 5 seconds"
        ));
    }
    let now = Utc::now();
    let next_running_time = now + interval;
    Ok(DbPayBatchCycle {
        owner_id,
        created_ts: now.adapt(),
        updated_ts: now.adapt(),
        cycle_interval: Some(interval.adapt()),
        cycle_cron: None,
        cycle_last_process: None,
        cycle_next_process: next_running_time.adapt(),
        cycle_max_interval: interval.adapt(),
        cycle_max_pay_time: (interval + extra_time_for_payment).adapt(),
    })
}
pub fn createBatchCycleBasedOnCron(
    owner_id: String,
    cron_str: &str,
    extra_time_for_payment: Duration,
) -> anyhow::Result<DbPayBatchCycle> {
    let parsed_cron = cron::Schedule::from_str(cron_str)
        .map_err(|err|anyhow!("Failed to parse cron expression: {} {}", cron_str, err))?;
//    createBatchCycleBasedOnInterval(owner_id, parsed_cron.upcoming(Utc::now()).unwrap() - Utc::now(), extra_time_for_payment);
    todo!()
}

#[derive(Queryable, Debug, Insertable, AsChangeset)]
#[table_name = "pay_batch_cycle"]
pub struct DbPayBatchCycleWriteObj {
    pub owner_id: NodeId,
    pub created_ts: TimestampAdapter,
    pub updated_ts: TimestampAdapter,
    pub cycle_cron: String,
    pub cycle_last_process: Option<TimestampAdapter>,
    pub cycle_next_process: TimestampAdapter,
    pub cycle_max_interval: String,
    pub cycle_max_pay_time: String,
}
