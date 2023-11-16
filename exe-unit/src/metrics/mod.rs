use std::fmt::Debug;
use std::ops::Not;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use std::{fs, thread};

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
    fn set(&mut self, _value: MetricData) {}
}

#[derive(Default)]
pub struct CpuMetric {}

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

#[derive(Default)]
pub struct MemMetric {
    peak: MetricData,
}

impl MemMetric {
    pub const ID: &'static str = "golem.usage.gib";
    pub const INF: &'static str = "mem.gib";

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

pub struct StorageMetric {
    path: PathBuf,
    peak: MetricData,
    last: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    interval: Duration,
}

impl StorageMetric {
    pub const ID: &'static str = "golem.usage.storage_gib";
    pub const INF: &'static str = "storage.gib";

    pub fn new(path: PathBuf, interval: Duration) -> Self {
        StorageMetric {
            path,
            peak: 0 as MetricData,
            last: Arc::new(AtomicU64::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            interval,
        }
    }

    fn spawn(&self) {
        let interval = self.interval;
        let path = self.path.clone();
        let last = self.last.clone();
        let running = self.running.clone();

        thread::spawn(move || {
            if running.load(Ordering::Relaxed).not() {
                return;
            }

            let (size, skipped) = match fs::read_dir(path.clone()) {
                Ok(dir) => Self::read_dir_size(dir),
                Err(err) => {
                    log::error!("StorageMetric: unable to read '{:?}': {:?}", path, err);
                    return;
                }
            };
            if skipped > c0 {
                log::warn!("StorageMetric: skipped {} filesystem entries", skipped);
            }

            last.store(size, Ordering::Relaxed);
            thread::sleep(interval);
        });
    }

    #[inline]
    fn read_dir_size(dir: fs::ReadDir) -> (u64, usize) {
        dir.fold((0, 0), |(sz, sk), file| {
            let (size, skipped) = Self::read_file_size(file).unwrap_or((0u64, 1));
            (sz + size, sk + skipped)
        })
    }

    #[inline]
    fn read_file_size(file: std::io::Result<fs::DirEntry>) -> std::io::Result<(u64, usize)> {
        let file = file?;
        let (size, skipped) = match file.metadata()? {
            data if data.is_dir() => Self::read_dir_size(fs::read_dir(file.path())?),
            data => (data.len(), 0),
        };
        Ok((size, skipped))
    }

    #[inline]
    fn update_peak(&mut self, val: MetricData) -> MetricData {
        if val > self.peak {
            self.peak = val;
        }
        self.peak
    }
}

impl Metric for StorageMetric {
    fn frame(&mut self) -> Result<MetricData> {
        if self.running.load(Ordering::Relaxed).not() {
            self.running.store(true, Ordering::Relaxed);
            self.spawn();
        }

        let val = self.last.load(Ordering::Relaxed) as MetricData / (1024. * 1024. * 1024.);
        self.update_peak(val);
        Ok(val)
    }

    #[inline]
    fn peak(&mut self) -> Result<MetricData> {
        Ok(self.peak)
    }
}

impl Drop for StorageMetric {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
