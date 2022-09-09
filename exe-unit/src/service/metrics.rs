#![allow(unused)]

use crate::error::Error;
use crate::message::{GetMetrics, SetMetric, Shutdown};
use crate::metrics::error::MetricError;
use crate::metrics::{
    CpuMetric, MemMetric, Metric, MetricData, MetricReport, StorageMetric, TimeMetric,
};
use crate::ExeUnitContext;
use actix::prelude::*;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct MetricsService {
    usage_vector: Vec<String>,
    metrics: HashMap<String, MetricProvider>,
}

impl MetricsService {
    pub fn try_new(
        ctx: &ExeUnitContext,
        backlog_limit: Option<usize>,
        supervise_caps: bool,
    ) -> Result<Self, MetricError> {
        let caps = move |ctx: &ExeUnitContext, id: &str| match supervise_caps {
            true => ctx.agreement.usage_limits.get(id).cloned(),
            _ => None,
        };

        let mut metrics = Self::metrics(ctx, backlog_limit, caps);
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

        Ok(MetricsService {
            usage_vector: ctx.agreement.usage_vector.clone(),
            metrics,
        })
    }

    #[cfg(feature = "sgx")]
    pub fn usage_vector() -> Vec<String> {
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
}

impl Actor for MetricsService {
    type Context = Context<Self>;
}

impl Handler<Shutdown> for MetricsService {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}

impl Handler<GetMetrics> for MetricsService {
    type Result = <GetMetrics as Message>::Result;

    fn handle(&mut self, _: GetMetrics, _: &mut Self::Context) -> Self::Result {
        let mut metrics = vec![0f64; self.usage_vector.len()];

        for (i, name) in self.usage_vector.iter().enumerate() {
            let metric = self
                .metrics
                .get_mut(name)
                .ok_or_else(|| MetricError::Unsupported(name.to_string()))?;

            let report = metric.report();
            metric.log_report(report.clone());

            match report {
                MetricReport::Frame(data) => metrics[i] = data,
                MetricReport::Error(error) => return Err(error.into()),
                MetricReport::LimitExceeded(data) => {
                    return Err(Error::UsageLimitExceeded(format!(
                        "{:?} exceeded the value of {:?}",
                        name, data
                    )))
                }
            }
        }

        Ok::<_, Error>(metrics)
    }
}

impl Handler<SetMetric> for MetricsService {
    type Result = ();

    fn handle(&mut self, msg: SetMetric, ctx: &mut Self::Context) -> Self::Result {
        match self.metrics.get_mut(&msg.name) {
            Some(provider) => provider.metric.set(msg.value),
            None => log::debug!("Unknown metric: {}", msg.name),
        }
    }
}

#[derive(Default)]
struct CustomMetric {
    val: MetricData,
    peak: MetricData,
}

impl Metric for CustomMetric {
    fn frame(&mut self) -> Result<MetricData, MetricError> {
        Ok(self.val)
    }

    fn peak(&mut self) -> Result<MetricData, MetricError> {
        Ok(self.peak)
    }

    fn set(&mut self, val: MetricData) {
        if val > self.peak {
            self.peak = val;
        }
        self.val = val;
    }
}

//TODO rafał
#[allow(clippy::type_complexity)]
struct MetricProvider {
    metric: Box<dyn Metric>,
    backlog: Arc<Mutex<VecDeque<(DateTime<Utc>, MetricReport)>>>,
    backlog_limit: Option<usize>,
    usage_limit: Option<MetricData>,
}

impl MetricProvider {
    pub fn new<M: Metric + 'static>(
        metric: M,
        backlog_limit: Option<usize>,
        usage_limit: Option<MetricData>,
    ) -> Self {
        MetricProvider {
            metric: Box::new(metric),
            backlog: Arc::new(Mutex::new(VecDeque::new())),
            backlog_limit,
            usage_limit,
        }
    }
}

impl MetricProvider {
    fn report(&mut self) -> MetricReport {
        if let Ok(data) = self.metric.peak() {
            if let Some(limit) = &self.usage_limit {
                if data > *limit {
                    return MetricReport::LimitExceeded(data);
                }
            }
        }

        match self.metric.frame() {
            Ok(data) => MetricReport::Frame(data),
            Err(error) => MetricReport::Error(error),
        }
    }

    fn log_report(&mut self, report: MetricReport) {
        let mut backlog = self.backlog.lock().unwrap();
        if let Some(limit) = self.backlog_limit {
            if backlog.len() == limit {
                backlog.pop_back();
            }
        }
        backlog.push_front((Utc::now(), report));
    }
}
