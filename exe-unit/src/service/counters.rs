#[allow(unused_imports)]
use crate::counters::{CpuMetric, MemMetric, StorageMetric};
use crate::ExeUnitContext;

use ya_counters::counters::TimeMetric;
use ya_counters::error::MetricError;
use ya_counters::service::{CustomMetric, MetricProvider, MetricsService};

use std::collections::HashMap;
#[allow(unused_imports)]
use std::time::Duration;

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
    let custom_metrics = ctx
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
pub fn usage_vector() -> Vec<String> {
    vec![TimeMetric::ID.to_string()]
}

#[cfg(feature = "sgx")]
fn metrics<F: Fn(&ExeUnitContext, &str) -> Option<f64>>(
    ctx: &ExeUnitContext,
    _backlog_limit: Option<usize>,
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
pub fn usage_vector() -> Vec<String> {
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
