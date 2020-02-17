use crate::commands::{MetricReportReq, MetricReportRes, Shutdown};
use crate::metrics::{Metric, MetricReport};
use crate::service::Service;
use actix::prelude::*;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct MetricService<M: Metric + 'static> {
    metric: M,
    backlog: Arc<Mutex<VecDeque<(DateTime<Utc>, MetricReport<M>)>>>,
    backlog_limit: Option<usize>,
    usage_limit: Option<<M as Metric>::Data>,
}

impl<M: Metric + 'static> From<M> for MetricService<M> {
    fn from(metric: M) -> Self {
        MetricService::new(metric)
    }
}

impl<M: Metric + 'static> MetricService<M> {
    pub fn new(metric: M) -> Self {
        MetricService {
            metric,
            backlog: Arc::new(Mutex::new(VecDeque::new())),
            backlog_limit: None,
            usage_limit: None,
        }
    }

    pub fn usage_limit(mut self, limit: <M as Metric>::Data) -> Self {
        self.usage_limit = Some(limit);
        self
    }

    pub fn backlog_limit(mut self, limit: usize) -> Self {
        self.backlog_limit = Some(limit);
        self
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

impl<M: Metric + Unpin + 'static> Service for MetricService<M> {}

impl<M: Metric + Unpin + 'static> Actor for MetricService<M> {
    type Context = Context<Self>;
}

impl<M: Metric + Unpin + 'static> Handler<MetricReportReq<M>> for MetricService<M> {
    type Result = <MetricReportReq<M> as Message>::Result;

    fn handle(&mut self, _: MetricReportReq<M>, ctx: &mut Self::Context) -> Self::Result {
        let report = self.report();
        self.log_report(report.clone());
        MetricReportRes(report)
    }
}

impl<M: Metric + Unpin + 'static> Handler<Shutdown> for MetricService<M> {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}
