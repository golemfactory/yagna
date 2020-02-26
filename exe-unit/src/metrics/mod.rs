use std::fmt::Debug;
use std::time::Duration;

pub mod error;
mod os;

pub type Result<T> = std::result::Result<T, error::MetricError>;

#[derive(Clone, Debug)]
pub enum MetricReport<M: Metric> {
    Frame(<M as Metric>::Data),
    Error(error::MetricError),
    LimitExceeded(<M as Metric>::Data),
}

pub trait MetricData: Clone + Debug + PartialOrd + Unpin + Send {
    fn as_f64(&self) -> f64;
}

pub trait Metric: Clone + Send {
    const ID: &'static str;
    type Data: MetricData;

    fn frame(&mut self) -> Result<Self::Data>;
    fn peak(&mut self) -> Result<Self::Data>;
}

#[derive(Clone, Debug)]
pub struct CpuMetric;

impl Metric for CpuMetric {
    const ID: &'static str = "CPU";
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

impl Default for CpuMetric {
    fn default() -> Self {
        CpuMetric {}
    }
}

impl MetricData for Duration {
    fn as_f64(&self) -> f64 {
        self.as_secs_f64()
    }
}

#[derive(Clone, Debug)]
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

impl Metric for MemMetric {
    const ID: &'static str = "RAM";
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

impl Default for MemMetric {
    fn default() -> Self {
        MemMetric { peak: 0i64 }
    }
}

impl MetricData for i64 {
    fn as_f64(&self) -> f64 {
        *self as f64
    }
}
