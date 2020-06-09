use nix::libc;
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::hash::Hash;
use std::mem;
use std::time::{Duration, Instant};
use thiserror::Error;

#[cfg(target_os = "macos")]
use libproc::libproc::bsd_info::BSDInfo;
#[cfg(target_os = "macos")]
use libproc::libproc::proc_pid::{listpids, pidinfo, ProcType};
use std::collections::HashSet;

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

#[derive(Clone, Debug)]
pub struct Process {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
    pub start_ts: u64,
}

#[cfg(target_os = "linux")]
impl Process {
    pub fn group(group: i32) -> impl Iterator<Item = Process> {
        use std::str::FromStr;

        std::fs::read_dir("/proc/")
            .expect("no /proc mountpoint")
            .into_iter()
            .filter_map(|res| res.ok())
            .filter_map(|entry| i32::from_str(&entry.file_name().to_string_lossy()).ok())
            .filter_map(|pid| Process::info(pid).ok())
            .filter(move |proc| proc.pgid == group)
    }

    pub fn info(pid: i32) -> Result<Process, SystemError> {
        let proc = Self::stat(pid)?;

        Ok(Process {
            pid: proc.pid,
            ppid: proc.stat.ppid,
            pgid: proc.stat.pgrp,
            start_ts: proc.stat.starttime,
        })
    }

    pub fn usage(pid: i32) -> Result<Usage, SystemError> {
        let proc = Self::stat(pid)?;

        let tps = procfs::ticks_per_second().map_err(|e| SystemError::Error(e.to_string()))?;
        let cpu_sec = Duration::from_secs((proc.stat.stime + proc.stat.utime) / tps as u64);
        let rss_gib = proc.stat.rss as f64 / (1024. * 1024.);

        Ok(Usage { cpu_sec, rss_gib })
    }

    #[inline]
    fn stat(pid: i32) -> Result<procfs::process::Process, SystemError> {
        procfs::process::Process::new(pid).map_err(|e| SystemError::Error(e.to_string()).into())
    }
}

#[cfg(target_os = "macos")]
impl Process {
    pub fn group(pgid: i32) -> impl Iterator<Item = Process> {
        listpids(ProcType::ProcAllPIDS)
            .unwrap_or_else(|_| Vec::new())
            .into_iter()
            .filter_map(|p| Process::info(p as i32).ok())
            .filter(move |p| p.pgid == pgid)
    }

    pub fn info(pid: i32) -> Result<Process, SystemError> {
        let info = pidinfo::<BSDInfo>(pid, 0).map_err(SystemError::Error)?;
        let start_ts = (info.pbi_start_tvsec + info.pbi_start_tvusec / 1_000_000_000) as u64;

        Ok(Process {
            pid: info.pbi_pid as i32,
            ppid: info.pbi_ppid as i32,
            pgid: info.pbi_pgid as i32,
            start_ts,
        })
    }

    pub fn usage(pid: i32) -> Result<Usage, SystemError> {
        use libproc::libproc::pid_rusage::{pidrusage, RUsageInfoV2};

        let usage = pidrusage::<RUsageInfoV2>(pid).map_err(SystemError::Error)?;
        let cpu_sec = Duration::from_secs_f64(
            (usage.ri_system_time as f64 + usage.ri_user_time as f64) / 1_000_000_000.,
        );
        let rss_gib = usage.ri_resident_size as f64 / (1024. * 1024. * 1024.);

        Ok(Usage { cpu_sec, rss_gib })
    }
}

#[derive(Clone, Debug)]
pub struct ProcessTree {
    pub pid: i32,
    proc: Process,
}

impl PartialEq for ProcessTree {
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid
    }
}

impl Eq for ProcessTree {}

impl Hash for ProcessTree {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(&self.pid.to_be_bytes())
    }
}

impl ProcessTree {
    pub fn try_new(pid: u32) -> Result<Self, SystemError> {
        let pid = pid as i32;
        let proc = Process::info(pid)?;
        Ok(ProcessTree { pid, proc })
    }

    #[inline]
    pub fn list(&self) -> Vec<Process> {
        let parents = parents(self.proc.pid);
        Process::group(self.proc.pgid)
            .filter(move |p| !parents.contains(&p.pid))
            .collect()
    }

    pub async fn kill(self, timeout: i64) -> Result<(), SystemError> {
        futures::future::join_all(self.list().into_iter().map(|p| kill(p.pid, timeout))).await;
        Ok(())
    }
}

pub struct Usage {
    pub cpu_sec: Duration,
    pub rss_gib: f64,
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

pub fn getrusage(resource: i32) -> Result<Usage, SystemError> {
    let mut usage = mem::MaybeUninit::<libc::rusage>::uninit();
    let ret = unsafe { libc::getrusage(resource as i32, usage.as_mut_ptr()) };
    match ret {
        0 => Ok(Usage::from(unsafe { usage.assume_init() })),
        _ => Err(SystemError::from(nix::Error::last()).into()),
    }
}

async fn kill(pid: i32, timeout: i64) {
    fn alive(pid: Pid) -> bool {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(status) => match status {
                WaitStatus::Exited(_, _) | WaitStatus::Signaled(_, _, _) => false,
                _ => true,
            },
            _ => false,
        }
    }

    let pid = Pid::from_raw(pid);
    let delay = Duration::from_secs_f32(timeout as f32 / 5.);
    let started = Instant::now();

    if let Ok(_) = signal::kill(pid, signal::Signal::SIGTERM) {
        log::info!("Sent SIGTERM to {:?}", pid);
        loop {
            if !alive(pid) {
                break;
            }
            if Instant::now() >= started + delay {
                log::info!("Sending SIGKILL to {:?}", pid);
                if let Ok(_) = signal::kill(pid, signal::Signal::SIGKILL) {
                    let _ = waitpid(pid, None);
                }
                break;
            }
            tokio::time::delay_for(delay).await;
        }
    }
}

fn parents(pid: i32) -> HashSet<i32> {
    let mut parents = HashSet::new();
    let mut proc = match Process::info(pid) {
        Ok(proc) => proc,
        _ => return parents,
    };

    while proc.ppid != 0 {
        parents.insert(proc.ppid);
        proc = match Process::info(proc.ppid) {
            Ok(p) => p,
            _ => break,
        };
        if proc.ppid == proc.pgid {
            break;
        }
    }
    parents
}
