use crate::commands::{MetricsRequest, Shutdown};
use crate::error::{Error, LocalServiceError};
use crate::metrics::{CpuMetric, MemMetric, Metric, MetricData, MetricReport};
use actix::prelude::*;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct MetricsService {
    cpu: MetricService<CpuMetric>,
    mem: MetricService<MemMetric>,
}

impl MetricsService {
    pub fn new(
        backlog_limit: Option<usize>,
        cpu_usage_limit: Option<<CpuMetric as Metric>::Data>,
        mem_usage_limit: Option<<MemMetric as Metric>::Data>,
    ) -> Self {
        let cpu = MetricService::new(
            CpuMetric::default(),
            backlog_limit.clone(),
            cpu_usage_limit.clone(),
        );
        let mem = MetricService::new(
            MemMetric::default(),
            backlog_limit.clone(),
            mem_usage_limit.clone(),
        );
        MetricsService { cpu, mem }
    }
}

impl Default for MetricsService {
    fn default() -> Self {
        MetricsService::new(None, None, None)
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

macro_rules! parse_report {
    ($metric:expr, $report:expr) => {
        match $report {
            MetricReport::Frame(data) => Ok(data.as_f64()),
            MetricReport::Error(error) => Err(LocalServiceError::MetricError(error).into()),
            MetricReport::LimitExceeded(data) => {
                let msg = format!("{:?} usage exceeded: {:?}", $metric, data.as_f64());
                Err(Error::UsageLimitExceeded(msg))
            }
        }
    };
}

impl Handler<MetricsRequest> for MetricsService {
    type Result = <MetricsRequest as Message>::Result;

    fn handle(&mut self, _: MetricsRequest, _: &mut Self::Context) -> Self::Result {
        let cpu_report = self.cpu.report();
        self.cpu.log_report(cpu_report.clone());
        let cpu_data: f64 = parse_report!(CpuMetric::ID, cpu_report)?;

        let mem_report = self.mem.report();
        self.mem.log_report(mem_report.clone());
        let mem_data: f64 = parse_report!(MemMetric::ID, mem_report)?;

        Ok(vec![cpu_data, mem_data])
    }
}

#[derive(Clone)]
struct MetricService<M: Metric + 'static> {
    metric: M,
    backlog: Arc<Mutex<VecDeque<(DateTime<Utc>, MetricReport<M>)>>>,
    backlog_limit: Option<usize>,
    usage_limit: Option<<M as Metric>::Data>,
}

impl<M: Metric + 'static> From<M> for MetricService<M> {
    fn from(metric: M) -> Self {
        MetricService::new(metric, None, None)
    }
}

impl<M: Metric + 'static> MetricService<M> {
    pub fn new(
        metric: M,
        backlog_limit: Option<usize>,
        usage_limit: Option<<M as Metric>::Data>,
    ) -> Self {
        MetricService {
            metric,
            backlog: Arc::new(Mutex::new(VecDeque::new())),
            backlog_limit,
            usage_limit,
        }
    }
}

impl<M: Metric> MetricService<M> {
    pub fn report(&mut self) -> MetricReport<M> {
        if let Ok(data) = self.metric.peak() {
            if let Some(limit) = &self.usage_limit {
                if &data > limit {
                    return MetricReport::LimitExceeded(data);
                }
            }
        }

        match self.metric.frame() {
            Ok(data) => MetricReport::Frame(data),
            Err(error) => MetricReport::Error(error),
        }
    }

    pub fn log_report(&mut self, report: MetricReport<M>) {
        let mut backlog = self.backlog.lock().unwrap();
        if let Some(limit) = self.backlog_limit {
            if backlog.len() == limit {
                backlog.pop_back();
            }
        }
        backlog.push_front((Utc::now(), report));
    }
}
