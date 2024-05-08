use std::ops::Not;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, thread};

use crate::counters::{Counter, CounterData};
use crate::error::CounterError;
use crate::os;
use crate::Result;

#[derive(Default)]
pub struct CpuCounter {}

impl CpuCounter {
    pub const ID: &'static str = "golem.usage.cpu_sec";
}

impl Counter for CpuCounter {
    #[inline]
    fn frame(&mut self) -> Result<CounterData> {
        os::cpu_time().map(|d| d.as_secs_f64())
    }

    #[inline]
    fn peak(&mut self) -> Result<CounterData> {
        self.frame()
    }
}

#[derive(Default)]
pub struct MemCounter {
    peak: CounterData,
}

impl MemCounter {
    pub const ID: &'static str = "golem.usage.gib";
    pub const INF: &'static str = "mem.gib";

    fn update_peak(&mut self, val: CounterData) -> CounterData {
        if val > self.peak {
            self.peak = val;
        }
        self.peak
    }
}

impl Counter for MemCounter {
    fn frame(&mut self) -> Result<CounterData> {
        match os::mem_rss() {
            Ok(data) => {
                let data = data as CounterData;
                self.update_peak(data);
                Ok(data)
            }
            Err(err) => match &err {
                CounterError::Unsupported(_) => self.peak(),
                _ => Err(err),
            },
        }
    }

    fn peak(&mut self) -> Result<CounterData> {
        let peak = os::mem_peak_rss()? as CounterData;
        Ok(self.update_peak(peak))
    }
}

pub struct StorageCounter {
    path: PathBuf,
    peak: CounterData,
    last: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    interval: Duration,
}

impl StorageCounter {
    pub const ID: &'static str = "golem.usage.storage_gib";
    pub const INF: &'static str = "storage.gib";

    pub fn new(path: PathBuf, interval: Duration) -> Self {
        StorageCounter {
            path,
            peak: 0 as CounterData,
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
                    log::error!("StorageCounter: unable to read '{:?}': {:?}", path, err);
                    return;
                }
            };
            if skipped > 0 {
                log::warn!("StorageCounter: skipped {} filesystem entries", skipped);
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
    fn update_peak(&mut self, val: CounterData) -> CounterData {
        if val > self.peak {
            self.peak = val;
        }
        self.peak
    }
}

impl Counter for StorageCounter {
    fn frame(&mut self) -> Result<CounterData> {
        if self.running.load(Ordering::Relaxed).not() {
            self.running.store(true, Ordering::Relaxed);
            self.spawn();
        }

        let val = self.last.load(Ordering::Relaxed) as CounterData / (1024. * 1024. * 1024.);
        self.update_peak(val);
        Ok(val)
    }

    #[inline]
    fn peak(&mut self) -> Result<CounterData> {
        Ok(self.peak)
    }
}

impl Drop for StorageCounter {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
