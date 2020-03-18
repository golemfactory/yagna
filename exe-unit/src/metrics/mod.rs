use std::fmt::Debug;
use std::time::SystemTime;

pub mod error;
mod os;

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
}

pub struct CpuMetric;

impl CpuMetric {
    pub const ID: &'static str = "golem.usage.cpu_sec";
}

impl Metric for CpuMetric {
    #[inline]
    fn frame(&mut self) -> Result<MetricData> {
        os::cpu_time().map(|d| d.as_secs_f64())
    }

    #[inline]
    fn peak(&mut self) -> Result<MetricData> {
        self.frame()
    }
}

impl Default for CpuMetric {
    fn default() -> Self {
        CpuMetric {}
    }
}

pub struct MemMetric {
    peak: MetricData,
}

impl MemMetric {
    pub const ID: &'static str = "golem.usage.gib";

    fn update_peak(&mut self, val: MetricData) -> MetricData {
        if val > self.peak {
            self.peak = val;
        }
        self.peak
    }
}

impl Metric for MemMetric {
    fn frame(&mut self) -> Result<MetricData> {
        match os::mem_rss() {
            Ok(data) => {
                let data = data as MetricData;
                self.update_peak(data);
                Ok(data)
            }
            Err(err) => match &err {
                error::MetricError::Unsupported(_) => self.peak(),
                _ => Err(err),
            },
        }
    }

    fn peak(&mut self) -> Result<MetricData> {
        let peak = os::mem_peak_rss()? as MetricData;
        Ok(self.update_peak(peak))
    }
}

impl Default for MemMetric {
    fn default() -> Self {
        MemMetric {
            peak: 0 as MetricData,
        }
    }
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
