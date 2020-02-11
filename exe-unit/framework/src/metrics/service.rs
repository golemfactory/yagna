use crate::metrics::{Metric, MetricReport};
use chrono::{DateTime, Utc};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct MetricService<M: Metric + 'static> {
    metric: M,
    backlog: Arc<Mutex<VecDeque<(DateTime<Utc>, MetricReport<M>)>>>,
    backlog_limit: Option<usize>,
    usage_limit: Option<<M as Metric>::Data>,
    metric_tx: Sender<MetricReport<M>>,
    metric_rx: Receiver<MetricReport<M>>,
    threads: Vec<Sender<()>>,
}

impl<M: Metric + 'static> From<M> for MetricService<M> {
    fn from(metric: M) -> Self {
        MetricService::new(metric)
    }
}

impl<M: Metric + 'static> MetricService<M> {
    pub fn new(metric: M) -> Self {
        let (metric_tx, metric_rx) = unbounded();

        MetricService {
            metric,
            backlog: Arc::new(Mutex::new(VecDeque::new())),
            backlog_limit: None,
            usage_limit: None,
            metric_tx,
            metric_rx,
            threads: Vec::new(),
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

impl<M: Metric + 'static> MetricService<M> {
    #[inline]
    pub fn receiver(&self) -> Receiver<MetricReport<M>> {
        self.metric_rx.clone()
    }

    #[inline]
    pub fn backlog(&self) -> Arc<Mutex<VecDeque<(DateTime<Utc>, MetricReport<M>)>>> {
        self.backlog.clone()
    }

    #[inline]
    pub fn tail(&self) -> Option<(DateTime<Utc>, MetricReport<M>)> {
        let backlog = self.backlog.lock().unwrap();
        backlog.front().cloned()
    }
}

impl<M: Metric + 'static> MetricService<M> {
    pub fn spawn(&mut self, report_interval: Duration) -> (thread::JoinHandle<()>, Sender<()>) {
        let mut service = self.clone();
        let (tx, rx) = unbounded();

        let handle = thread::spawn(move || loop {
            let started = Instant::now();
            if let Ok(_) = rx.try_recv() {
                break;
            }

            let report = service.report();
            service.log_report(report.clone());
            service.tx_report(report);

            let dt = report_interval - (Instant::now() - started);
            thread::sleep(dt);
        });

        self.threads.push(tx.clone());
        (handle, tx)
    }

    pub fn stop(&mut self) {
        let threads = std::mem::replace(&mut self.threads, vec![]);
        for tx in threads.into_iter() {
            if let Err(e) = tx.send(()) {
                log::error!("Unable to stop the thread: {:?}", e);
            }
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
            Err(error) => MetricReport::FrameError(error),
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

    #[inline]
    pub fn tx_report(&self, report: MetricReport<M>) {
        if let Err(e) = self.metric_tx.send(report) {
            log::warn!("Unable to send a report: {:?}", e);
        }
    }
}
