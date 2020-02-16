use crate::metrics::error::MetricError;
use chrono::{DateTime, Utc};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub mod error;
mod os;
pub mod service;

pub type Result<T> = std::result::Result<T, error::MetricError>;

#[derive(Clone, Debug)]
pub enum MetricReport<M: Metric> {
    Frame(<M as Metric>::Data),
    FrameError(error::MetricError),
    LimitExceeded(<M as Metric>::Data),
}

pub trait Metric: Clone + Send {
    type Data: Clone + Debug + PartialOrd + Send;

    fn frame(&mut self) -> Result<Self::Data>;
    fn peak(&mut self) -> Result<Self::Data>;
}

#[derive(Clone)]
pub struct CpuMetric;

impl Default for CpuMetric {
    fn default() -> Self {
        CpuMetric {}
    }
}

impl Metric for CpuMetric {
    type Data = Duration;

    #[inline]
    fn frame(&mut self) -> Result<Self::Data> {
        os::cpu_time()
    }

    #[inline]
    fn peak(&mut self) -> Result<Self::Data> {
        self.frame()
    }
}

#[derive(Clone)]
pub struct MemMetric {
    peak: <Self as Metric>::Data,
}

impl MemMetric {
    fn update_peak(&mut self, val: <Self as Metric>::Data) -> <Self as Metric>::Data {
        if val > self.peak {
            self.peak = val;
        }
        self.peak
    }
}

impl Default for MemMetric {
    fn default() -> Self {
        MemMetric { peak: 0i64 }
    }
}

impl Metric for MemMetric {
    type Data = i64;

    fn frame(&mut self) -> Result<Self::Data> {
        match os::mem_rss() {
            Ok(data) => {
                self.update_peak(data);
                Ok(data)
            }
            Err(err) => match &err {
                error::MetricError::Unsupported => self.peak(),
                _ => Err(err),
            },
        }
    }

    fn peak(&mut self) -> Result<Self::Data> {
        let peak = os::mem_peak_rss()?;
        Ok(self.update_peak(peak))
    }
}
