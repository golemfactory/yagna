#![allow(unused)]

use crate::counters::{Metric, MetricData, MetricReport};
use crate::error::MetricError;
use crate::message::{GetMetrics, SetMetric, Shutdown};

use actix::prelude::*;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ya_agreement_utils::AgreementView;

pub struct MetricsServiceBuilder {
    usage_vector: Vec<String>,
    backlog_limit: Option<usize>,
    usage_limits: Option<HashMap<String, f64>>,
    metrics: HashMap<String, Box<dyn Metric>>,
}

impl MetricsServiceBuilder {
    pub fn new(usage_vector: Vec<String>, backlog_limit: Option<usize>) -> Self {
        let metrics = Default::default();
        Self {
            usage_vector,
            backlog_limit,
            usage_limits: None,
            metrics,
        }
    }

    pub fn with_usage_limits(&mut self, usage_limit: HashMap<String, f64>) -> &mut Self {
        self.usage_limits = Some(usage_limit);
        self
    }

    pub fn with_metric(&mut self, metric_id: &str, metric: Box<dyn Metric>) -> &mut Self {
        // let metric_provider = MetricProvider::new(metric, Some(1));
        // overwriting an existing metric should not matter
        self.metrics.insert(metric_id.into(), metric);
        self
    }

    pub fn build(self) -> MetricsService {
        let custom_metrics_ids: Vec<String> = self
            .usage_vector
            .iter()
            .filter(|metric_id| !self.metrics.contains_key(*metric_id))
            .cloned()
            .collect();

        if !custom_metrics_ids.is_empty() {
            log::debug!("Custom metrics: {:?}", custom_metrics_ids)
        }

        let mut metrics = HashMap::new();
        let usage_limits = self.usage_limits.unwrap_or_default();

        for custom_metric_id in custom_metrics_ids {
            let usage_limit = usage_limits.get(&custom_metric_id).cloned();
            let metric = Box::<CustomMetric>::default();
            let provider = MetricProvider::new(metric, self.backlog_limit, usage_limit);
            metrics.insert(custom_metric_id, provider);
        }

        for (metric_id, metric) in self.metrics {
            let usage_limit = usage_limits.get(&metric_id).cloned();
            // non custom metrics have backlog limit set to 1
            let metric_provider = MetricProvider::new(metric, Some(1), usage_limit);
            metrics.insert(metric_id.clone(), metric_provider);
        }

        MetricsService::new(self.usage_vector, metrics)
    }
}

pub struct MetricsService {
    usage_vector: Vec<String>,
    metrics: HashMap<String, MetricProvider>,
}

impl MetricsService {
    pub fn new(usage_vector: Vec<String>, metrics: HashMap<String, MetricProvider>) -> Self {
        Self {
            usage_vector,
            metrics,
        }
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
                MetricReport::Error(error) => return Err(error),
                MetricReport::LimitExceeded(data) => {
                    return Err(MetricError::UsageLimitExceeded(format!(
                        "{:?} exceeded the value of {:?}",
                        name, data
                    )))
                }
            }
        }

        Ok::<_, MetricError>(metrics)
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
pub struct CustomMetric {
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

#[allow(clippy::type_complexity)]
pub struct MetricProvider {
    metric: Box<dyn Metric>,
    backlog: Arc<Mutex<VecDeque<(DateTime<Utc>, MetricReport)>>>,
    backlog_limit: Option<usize>,
    usage_limit: Option<MetricData>,
}

impl MetricProvider {
    pub fn new(
        metric: Box<dyn Metric>,
        backlog_limit: Option<usize>,
        usage_limit: Option<MetricData>,
    ) -> Self {
        MetricProvider {
            metric,
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
