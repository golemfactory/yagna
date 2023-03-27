use crate::metrics::{error::MetricError, Result};
use crate::process::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

lazy_static::lazy_static! {
    static ref METRICS: Arc<RwLock<Metrics>> = Arc::new(RwLock::new(Metrics::default()));
}

const MAX_UPDATE_RESOLUTION_MS: i64 = 100;

pub fn cpu_time() -> Result<Duration> {
    let mut metrics = (*METRICS).write().map_err(SystemError::from)?;
    metrics.sample()?;
    Ok(metrics.cpu_total)
}

#[inline(always)]
pub fn mem_rss() -> Result<f64> {
    Err(MetricError::Unsupported("mem".to_owned()))
}

pub fn mem_peak_rss() -> Result<f64> {
    let mut metrics = (*METRICS).write().map_err(SystemError::from)?;
    metrics.sample()?;
    Ok(metrics.mem_total)
}

struct Metrics {
    process_tree: ProcessTree,
    cpu: HashMap<i32, Duration>,
    mem: HashMap<i32, f64>,
    cpu_total: Duration,
    mem_total: f64,
    updated: i64,
}

impl Default for Metrics {
    fn default() -> Self {
        let pid = unsafe { nix::libc::getpid() } as u32;
        let process_tree = ProcessTree::try_new(pid).unwrap();

        Metrics {
            cpu: HashMap::new(),
            mem: HashMap::new(),
            cpu_total: Duration::default(),
            mem_total: 0f64,
            updated: 0i64,
            process_tree,
        }
    }
}

impl Metrics {
    fn sample(&mut self) -> Result<()> {
        // grace period
        let now = chrono::Local::now().timestamp_millis();
        if now < self.updated + MAX_UPDATE_RESOLUTION_MS {
            return Ok(());
        }
        self.updated = now;

        // read and store process tree usage
        self.extend(self.process_tree.list().into_iter());
        self.cpu_total = self.cpu.values().sum();
        self.mem_total = self.mem.values().sum();

        // apply corrections in case we skipped a process
        let usage = getrusage(0)? + getrusage(-1)?;

        if usage.cpu_sec > self.cpu_total {
            let dv = usage.cpu_sec - self.cpu_total;
            *self.cpu.entry(-1).or_insert_with(Duration::default) += dv;
            self.cpu_total += dv;
        }
        if usage.rss_gib > self.mem_total {
            let dv = usage.rss_gib - self.mem_total;
            *self.mem.entry(-1).or_insert(0f64) += dv;
            self.mem_total += dv;
        }

        Ok(())
    }

    fn extend<I: Iterator<Item = Process>>(&mut self, iter: I) {
        iter.filter_map(|proc| Process::usage(proc.pid).map(|usage| (proc.pid, usage)).ok())
            .for_each(|(pid, usage)| {
                let cpu_entry = self
                    .cpu
                    .entry(pid)
                    .or_insert_with(|| Duration::from_secs(0));
                let mem_entry = self.mem.entry(pid).or_insert(0f64);

                if usage.cpu_sec > *cpu_entry {
                    *cpu_entry = usage.cpu_sec;
                }
                if usage.rss_gib > *mem_entry {
                    *mem_entry = usage.rss_gib;
                }
            })
    }
}
