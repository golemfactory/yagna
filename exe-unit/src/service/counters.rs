#[allow(unused_imports)]
use crate::counters::{CpuMetric, MemMetric, StorageMetric};
use crate::ExeUnitContext;

use ya_counters::counters::{Metric, TimeMetric};
use ya_counters::service::{MetricsService, MetricsServiceBuilder};

use std::collections::HashMap;

#[allow(unused_imports)]
use std::time::Duration;

pub fn build(
    ctx: &ExeUnitContext,
    backlog_limit: Option<usize>,
    supervise_caps: bool,
) -> MetricsService {
    let mut builder = MetricsServiceBuilder::new(ctx.agreement.usage_vector.clone(), backlog_limit);

    if supervise_caps {
        builder.with_usage_limits(ctx.agreement.usage_limits.clone());
    }

    for (metric_id, metric) in metrics(ctx) {
        builder.with_metric(&metric_id, metric);
    }

    builder.build()
}

#[cfg(feature = "sgx")]
pub fn usage_vector() -> Vec<String> {
    vec![TimeMetric::ID.to_string()]
}

#[cfg(feature = "sgx")]
fn metrics(_ctx: &ExeUnitContext) -> HashMap<String, Box<dyn Metric>> {
    vec![(
        TimeMetric::ID.to_string(),
        Box::<TimeMetric>::default() as Box<dyn Metric>,
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
    .into_iter()
    .collect()
}

#[cfg(not(feature = "sgx"))]
fn metrics(ctx: &ExeUnitContext) -> HashMap<String, Box<dyn Metric>> {
    vec![
        (
            CpuMetric::ID.to_string(),
            Box::new(CpuMetric::default()) as Box<dyn Metric>,
        ),
        (
            MemMetric::ID.to_string(),
            Box::new(MemMetric::default()) as Box<dyn Metric>,
        ),
        (
            StorageMetric::ID.to_string(),
            Box::new(StorageMetric::new(
                ctx.work_dir.clone(),
                Duration::from_secs(60 * 5),
            )) as Box<dyn Metric>,
        ),
        (
            TimeMetric::ID.to_string(),
            Box::new(TimeMetric::default()) as Box<dyn Metric>,
        ),
    ]
    .into_iter()
    .collect()
}
