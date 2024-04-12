pub mod usage;

pub use usage::*;

use std::collections::HashSet;
use std::hash::Hash;
use std::io;
use std::time::{Duration, Instant};

use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use thiserror::Error;

#[cfg(target_os = "macos")]
use libproc::libproc::bsd_info::BSDInfo;
#[cfg(target_os = "macos")]
use libproc::libproc::proc_pid::{listpids, pidinfo, ProcType};

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

impl From<io::Error> for SystemError {
    fn from(e: io::Error) -> Self {
        SystemError::Error(format!("IO error: {e}"))
    }
}

#[derive(Clone, Debug)]
pub struct Process {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
}

#[cfg(target_os = "linux")]
impl Process {
    pub fn group(group: i32) -> impl Iterator<Item = Process> {
        use std::str::FromStr;

        std::fs::read_dir("/proc/")
            .expect("no /proc mountpoint")
            .filter_map(|res| res.ok())
            .filter_map(|entry| i32::from_str(&entry.file_name().to_string_lossy()).ok())
            .filter_map(|pid| Process::info(pid).ok())
            .filter(move |proc| proc.pgid == group)
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
        Ok(Process {
            pid: info.pbi_pid as i32,
            ppid: info.pbi_ppid as i32,
            pgid: info.pbi_pgid as i32,
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
        let parents = match parents(self.proc.pid) {
            Ok(parents) => parents,
            _ => return Vec::new(),
        };
        Process::group(self.proc.pgid)
            .filter(move |p| !parents.contains(&p.pid))
            .collect()
    }

    pub async fn kill(self, timeout: i64) -> Result<(), SystemError> {
        futures::future::join_all(self.list().into_iter().map(|p| kill(p.pid, timeout))).await;
        Ok(())
    }
}

pub async fn kill(pid: i32, timeout: i64) -> Result<(), SystemError> {
    fn alive(pid: Pid) -> Result<bool, SystemError> {
        Ok(!matches!(
            waitpid(pid, Some(WaitPidFlag::WNOHANG))?,
            WaitStatus::Exited(_, _) | WaitStatus::Signaled(_, _, _)
        ))
    }

    let pid = Pid::from_raw(pid);
    let delay = Duration::from_secs_f32(timeout as f32 / 5.);
    let started = Instant::now();

    signal::kill(pid, signal::Signal::SIGTERM)?;
    log::info!("Sent SIGTERM to {:?}", pid);
    loop {
        if !alive(pid)? {
            break;
        }
        if Instant::now() >= started + delay {
            log::info!("Sending SIGKILL to {:?}", pid);
            signal::kill(pid, signal::Signal::SIGKILL)?;
            waitpid(pid, None)?;
            break;
        }
        tokio::time::sleep(delay).await;
    }
    Ok(())
}

fn parents(pid: i32) -> Result<HashSet<i32>, SystemError> {
    let mut proc = Process::info(pid)?;
    let mut ancestors = HashSet::new();
    let pgid = proc.pgid;

    while proc.ppid > 1 {
        proc = match Process::info(proc.ppid) {
            Ok(proc) => proc,
            _ => break,
        };
        ancestors.insert(proc.pid);
        if proc.pid == pgid {
            break;
        }
    }
    Ok(ancestors)
}
