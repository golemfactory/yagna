use crate::counters::{CpuMetric, MemMetric, Metric, MetricData, MetricReport, StorageMetric};
use crate::error::Error;
use crate::message::{GetMetrics, SetMetric, Shutdown};
use crate::metrics::error::MetricError;
use crate::ExeUnitContext;
use actix::prelude::*;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ya_counters::counters::{Metric, TimeMetric};
use ya_counters::service::MetricsService;

pub fn try_new(
    ctx: &ExeUnitContext,
    backlog_limit: Option<usize>,
    supervise_caps: bool,
) -> Result<MetricsService, MetricError> {
    let caps = move |ctx: &ExeUnitContext, id: &str| match supervise_caps {
        true => ctx.agreement.usage_limits.get(id).cloned(),
        _ => None,
    };

    let mut metrics = metrics(ctx, backlog_limit, caps);
    let mut custom_metrics = ctx
        .agreement
        .usage_vector
        .iter()
        .filter(|e| !metrics.contains_key(*e))
        .cloned()
        .collect::<Vec<_>>();

    if !custom_metrics.is_empty() {
        log::debug!("Metrics provided by the runtime: {:?}", custom_metrics)
    }
    custom_metrics.into_iter().for_each(|m| {
        let caps = caps(ctx, &m);
        let provider = MetricProvider::new(CustomMetric::default(), backlog_limit, caps);
        metrics.insert(m, provider);
    });
    let usage_vector = ctx.agreement.usage_vector.clone();
    Ok(MetricsService::new(usage_vector, metrics))
}

#[cfg(feature = "sgx")]
fn usage_vector() -> Vec<String> {
    vec![TimeMetric::ID.to_string()]
}

#[cfg(feature = "sgx")]
fn metrics<F: Fn(&ExeUnitContext, &str) -> Option<f64>>(
    ctx: &ExeUnitContext,
    backlog_limit: Option<usize>,
    caps: F,
) -> HashMap<String, MetricProvider> {
    vec![(
        TimeMetric::ID.to_string(),
        MetricProvider::new(TimeMetric::default(), Some(1), caps(ctx, TimeMetric::ID)),
    )]
    .into_iter()
    .collect()
}

#[cfg(not(feature = "sgx"))]
fn usage_vector() -> Vec<String> {
    vec![
        TimeMetric::ID.to_string(),
        CpuMetric::ID.to_string(),
        MemMetric::ID.to_string(),
        StorageMetric::ID.to_string(),
    ]
}

#[cfg(not(feature = "sgx"))]
fn metrics<F: Fn(&ExeUnitContext, &str) -> Option<f64>>(
    ctx: &ExeUnitContext,
    backlog_limit: Option<usize>,
    caps: F,
) -> HashMap<String, MetricProvider> {
    vec![
        (
            CpuMetric::ID.to_string(),
            MetricProvider::new(
                CpuMetric::default(),
                backlog_limit,
                caps(ctx, CpuMetric::ID),
            ),
        ),
        (
            MemMetric::ID.to_string(),
            MetricProvider::new(
                MemMetric::default(),
                backlog_limit,
                caps(ctx, MemMetric::ID),
            ),
        ),
        (
            StorageMetric::ID.to_string(),
            MetricProvider::new(
                StorageMetric::new(ctx.work_dir.clone(), Duration::from_secs(60 * 5)),
                backlog_limit,
                caps(ctx, StorageMetric::ID),
            ),
        ),
        (
            TimeMetric::ID.to_string(),
            MetricProvider::new(TimeMetric::default(), Some(1), caps(ctx, TimeMetric::ID)),
        ),
    ]
    .into_iter()
    .collect()
}
