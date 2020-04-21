use crate::metrics::{error::MetricError, Result};
use nix::libc;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use thiserror::Error;

lazy_static::lazy_static! {
    static ref METRICS: Arc<RwLock<Metrics>> = Arc::new(RwLock::new(Metrics::default()));
    static ref PID: i32 = unsafe { libc::getpid() };
    static ref GID: i32 = unsafe { libc::getpgrp() };
}

const MAX_UPDATE_RESOLUTION_MS: i64 = 100;

#[derive(Clone, Debug, Error)]
pub enum SystemError {
    #[error("{0}")]
    NixError(#[from] nix::Error),
    #[error("unable to retrieve lock: {0}")]
    LockError(String),
    #[error("{0}")]
    Error(String),
}

impl<T> From<std::sync::PoisonError<T>> for SystemError {
    fn from(e: std::sync::PoisonError<T>) -> Self {
        SystemError::LockError(e.to_string())
    }
}

pub fn cpu_time() -> Result<Duration> {
    let mut metrics = (&(*METRICS)).write().map_err(SystemError::from)?;
    metrics.sample()?;
    Ok(metrics.cpu())
}

pub fn mem_rss() -> Result<f64> {
    Err(MetricError::Unsupported("mem".to_owned()))
}

pub fn mem_peak_rss() -> Result<f64> {
    let mut metrics = (&(*METRICS)).write().map_err(SystemError::from)?;
    metrics.sample()?;
    Ok(metrics.mem())
}

struct Metrics {
    cpu: HashMap<i32, Duration>,
    mem: HashMap<i32, f64>,
    cpu_total: Duration,
    mem_total: f64,
    updated: i64,
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics {
            cpu: HashMap::new(),
            mem: HashMap::new(),
            cpu_total: Duration::default(),
            mem_total: 0f64,
            updated: 0i64,
        }
    }
}

impl Metrics {
    fn sample(&mut self) -> Result<()> {
        // Grace period
        let now = chrono::Local::now().timestamp_millis();
        if now < self.updated + MAX_UPDATE_RESOLUTION_MS {
            return Ok(());
        }
        self.updated = now;

        // Read and store process tree usage
        self.extend(process_tree_info(*PID, *GID)?.into_iter());
        self.cpu_total = self.cpu.values().sum();
        self.mem_total = self.mem.values().sum();

        // Apply corrections in case there was a process we missed
        let usage = getrusage(0)? + getrusage(-1)?;

        if usage.cpu_sec > self.cpu_total {
            let dv = usage.cpu_sec - self.cpu_total;
            *self.cpu.entry(-1).or_insert_with(|| Duration::default()) += dv;
            self.cpu_total += dv;
        }

        if usage.rss_gib > self.mem_total {
            let dv = usage.rss_gib - self.mem_total;
            *self.mem.entry(-1).or_insert(0f64) += dv;
            self.mem_total += dv;
        }

        Ok(())
    }

    #[inline]
    fn cpu(&self) -> Duration {
        self.cpu_total
    }

    #[inline]
    fn mem(&self) -> f64 {
        self.mem_total
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

struct Process {
    pid: i32,
    ppid: i32,
    pgid: i32,
    start_ts: u64,
}

#[cfg(target_os = "linux")]
impl Process {
    fn all() -> impl Iterator<Item = Process> {
        use std::str::FromStr;

        std::fs::read_dir("/proc/")
            .expect("no /proc mountpoint")
            .into_iter()
            .filter_map(|res| res.ok())
            .filter_map(|entry| i32::from_str(&entry.file_name().to_string_lossy()).ok())
            .filter_map(|pid| Process::info(pid).ok())
    }

    fn info(pid: i32) -> Result<Process> {
        let proc = Self::stat(pid)?;

        Ok(Process {
            pid: proc.pid,
            ppid: proc.stat.ppid,
            pgid: proc.stat.pgrp,
            start_ts: proc.stat.starttime,
        })
    }

    fn usage(pid: i32) -> Result<Usage> {
        let proc = Self::stat(pid)?;

        let tps = procfs::ticks_per_second().map_err(|e| SystemError::Error(e.to_string()))?;
        let cpu_sec = Duration::from_secs((proc.stat.stime + proc.stat.utime) / tps as u64);
        let rss_gib = proc.stat.rss as f64 / (1024. * 1024.);

        Ok(Usage { cpu_sec, rss_gib })
    }

    #[inline]
    fn stat(pid: i32) -> Result<procfs::process::Process> {
        procfs::process::Process::new(pid).map_err(|e| SystemError::Error(e.to_string()).into())
    }
}

#[cfg(target_os = "macos")]
impl Process {
    fn all() -> impl Iterator<Item = Process> {
        use libproc::libproc::proc_pid::{listpids, ProcType};

        listpids(ProcType::ProcAllPIDS)
            .unwrap_or(Vec::new())
            .into_iter()
            .filter_map(|p| Process::info(p as i32).ok())
    }

    fn info(pid: i32) -> Result<Process> {
        use libproc::libproc::bsd_info::BSDInfo;
        use libproc::libproc::proc_pid::pidinfo;

        let info = pidinfo::<BSDInfo>(pid, 0).map_err(SystemError::Error)?;
        let start_ts = (info.pbi_start_tvsec + info.pbi_start_tvusec / 1_000_000_000) as u64;

        Ok(Process {
            pid: info.pbi_pid as i32,
            ppid: info.pbi_ppid as i32,
            pgid: info.pbi_pgid as i32,
            start_ts,
        })
    }

    fn usage(pid: i32) -> Result<Usage> {
        use libproc::libproc::pid_rusage::{pidrusage, RUsageInfoV2};

        let usage = pidrusage::<RUsageInfoV2>(pid).map_err(SystemError::Error)?;
        let cpu_sec = Duration::from_secs_f64(
            (usage.ri_system_time as f64 + usage.ri_user_time as f64) / 1_000_000_000.,
        );
        let rss_gib = usage.ri_resident_size as f64 / (1024. * 1024. * 1024.);

        Ok(Usage { cpu_sec, rss_gib })
    }
}

struct Usage {
    cpu_sec: Duration,
    rss_gib: f64,
}

impl std::ops::Add for Usage {
    type Output = Usage;

    fn add(self, rhs: Self) -> Self::Output {
        Usage {
            cpu_sec: self.cpu_sec + rhs.cpu_sec,
            rss_gib: self.rss_gib + rhs.rss_gib,
        }
    }
}

impl From<libc::rusage> for Usage {
    fn from(usage: libc::rusage) -> Self {
        let cpu_sec = Duration::from_secs((usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as u64)
            + Duration::from_micros((usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as u64);
        let rss_gib = (usage.ru_maxrss + usage.ru_ixrss + usage.ru_idrss + usage.ru_isrss) as f64
            / (1024. * 1024. * 1024.);

        Usage { cpu_sec, rss_gib }
    }
}

fn process_tree_info(pid: i32, gid: i32) -> Result<Vec<Process>> {
    let now = chrono::Local::now().timestamp() as u64;

    let candidates = Process::all()
        .filter(|i| (i.pgid == gid) && (i.start_ts <= now))
        .collect::<Vec<_>>();

    let procs = vec![Process::info(pid)?];
    let mut parents = HashSet::new();
    parents.insert(pid);

    process_info(candidates, parents, procs)
}

fn process_info(
    candidates: Vec<Process>,
    parents: HashSet<i32>,
    mut procs: Vec<Process>,
) -> Result<Vec<Process>> {
    let mut children = HashSet::new();

    let (pids, candidates): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|proc| parents.contains(&(proc.ppid)));

    pids.into_iter().for_each(|proc| {
        if !children.contains(&proc.pid) {
            children.insert(proc.pid);
            procs.push(proc);
        }
    });

    if children.is_empty() {
        Ok(procs)
    } else {
        process_info(candidates, children, procs)
    }
}

fn getrusage(resource: i32) -> Result<Usage> {
    let mut usage = mem::MaybeUninit::<libc::rusage>::uninit();
    let ret = unsafe { libc::getrusage(resource as i32, usage.as_mut_ptr()) };
    match ret {
        0 => Ok(Usage::from(unsafe { usage.assume_init() })),
        _ => Err(SystemError::from(nix::Error::last()).into()),
    }
}
