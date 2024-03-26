use std::fmt::Debug;
use std::ops::Not;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use std::{fs, thread};

use crate::error;

pub type Result<T> = std::result::Result<T, error::MetricError>;
pub type MetricData = f64;

#[derive(Clone, Debug)]
pub enum MetricReport {
    Frame(MetricData),
    LimitExceeded(MetricData),
    Error(error::MetricError),
}

pub trait Metric {
    fn frame(&mut self) -> Result<MetricData>;
    fn peak(&mut self) -> Result<MetricData>;
    fn set(&mut self, _value: MetricData) {}
}

pub struct TimeMetric {
    started: SystemTime,
}

impl TimeMetric {
    pub const ID: &'static str = "golem.usage.duration_sec";
}

impl Default for TimeMetric {
    fn default() -> Self {
        TimeMetric {
            started: SystemTime::now(),
        }
    }
}

impl Metric for TimeMetric {
    fn frame(&mut self) -> Result<MetricData> {
        Ok(SystemTime::now()
            .duration_since(self.started)?
            .as_secs_f64())
    }

    fn peak(&mut self) -> Result<MetricData> {
        self.frame()
    }
}
