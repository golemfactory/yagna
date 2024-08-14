use crate::schema::*;
use anyhow::anyhow;
use chrono::{Duration, Utc};
use cron::Schedule;
use serde::Serialize;
use std::str::FromStr;
use ya_client_model::NodeId;
use ya_persistence::types::{AdaptDuration, AdaptTimestamp, DurationAdapter, TimestampAdapter};

#[derive(Debug, Clone, Queryable, Insertable, AsChangeset, Serialize)]
#[serde(rename_all = "camelCase")]
#[primary_key(owner_id)]
#[table_name = "pay_batch_cycle"]
pub struct DbPayBatchCycle {
    pub owner_id: NodeId,
    pub platform: String,
    pub created_ts: TimestampAdapter,
    pub updated_ts: TimestampAdapter,
    pub cycle_interval: Option<DurationAdapter>,
    pub cycle_cron: Option<String>,
    pub cycle_last_process: Option<TimestampAdapter>,
    pub cycle_next_process: TimestampAdapter,
    pub cycle_max_interval: DurationAdapter,
    pub cycle_max_pay_time: DurationAdapter,
}

pub fn create_batch_cycle_based_on_interval(
    owner_id: NodeId,
    platform: String,
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
        platform,
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
pub fn parse_cron_str(cron_str: &str) -> anyhow::Result<Schedule> {
    let cron_len = cron_str.split(' ').filter(|s| !s.is_empty()).count();
    if cron_len < 5 {
        return Err(anyhow!("Invalid cron expression: {}", cron_str));
    };
    if cron_len > 6 {
        return Err(anyhow!("Invalid cron expression: {}", cron_str));
    };
    let cron_str = if cron_len == 5 {
        "0 ".to_string() + cron_str
    } else {
        cron_str.to_string()
    };

    Schedule::from_str(&cron_str)
        .map_err(|err| anyhow!("Failed to parse cron expression: {} {}", cron_str, err))
}

pub fn create_batch_cycle_based_on_cron(
    owner_id: NodeId,
    platform: String,
    cron_str: &str,
    extra_time_for_payment: Duration,
) -> anyhow::Result<DbPayBatchCycle> {
    let schedule = parse_cron_str(cron_str)
        .map_err(|err| anyhow!("Failed to parse cron expression: {} {}", cron_str, err))?;

    let mut max_interval_s = 0;
    //check max interval

    let mut prev_ms = None;
    for dt in schedule.upcoming(Utc).take(100) {
        let next = dt.timestamp_millis() / 1000;
        if let Some(prev) = prev_ms {
            let diff = next - prev;
            if diff > max_interval_s {
                max_interval_s = diff;
            }
        }
        prev_ms = Some(next);
    }
    let max_interval = Duration::seconds(max_interval_s);
    let now = Utc::now();
    let next_running_time = schedule.upcoming(Utc).next().ok_or(anyhow!(
        "Failed to get next running time for cron expression: {}",
        cron_str
    ))?;

    Ok(DbPayBatchCycle {
        owner_id,
        platform,
        created_ts: now.adapt(),
        updated_ts: now.adapt(),
        cycle_interval: None,
        cycle_cron: Some(cron_str.to_string()),
        cycle_last_process: None,
        cycle_next_process: next_running_time.adapt(),
        cycle_max_interval: max_interval.adapt(),
        cycle_max_pay_time: (max_interval + extra_time_for_payment).adapt(),
    })
}
