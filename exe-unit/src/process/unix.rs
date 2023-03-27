use nix::libc;
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashSet;
use std::hash::Hash;
use std::io;
use std::mem;
use std::time::{Duration, Instant};
use thiserror::Error;

#[cfg(target_os = "linux")]
use nix::unistd::sysconf;
#[cfg(target_os = "linux")]
use nix::unistd::SysconfVar::CLK_TCK;

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
            .into_iter()
            .filter_map(|res| res.ok())
            .filter_map(|entry| i32::from_str(&entry.file_name().to_string_lossy()).ok())
            .filter_map(|pid| Process::info(pid).ok())
            .filter(move |proc| proc.pgid == group)
    }

    pub fn info(pid: i32) -> Result<Process, SystemError> {
        let stat = StatStub::read(pid)?;
        Ok(Process {
            pid: stat.pid,
            ppid: stat.ppid,
            pgid: stat.pgid,
        })
    }

    pub fn usage(pid: i32) -> Result<Usage, SystemError> {
        let stat = StatStub::read(pid)?;
        let tps = Self::ticks_per_second()?;

        let cpu_sec = Duration::from_secs((stat.stime + stat.utime) / tps as u64);
        let rss_gib = stat.rss as f64 / (1024. * 1024.);

        Ok(Usage { cpu_sec, rss_gib })
    }

    fn ticks_per_second() -> Result<i64, SystemError> {
        match sysconf(CLK_TCK) {
            Ok(Some(tps)) => Ok(tps),
            Ok(None) => Err(nix::errno::Errno::ENOTSUP.into()),
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq, Default)]
struct StatStub {
    pub pid: i32,
    pub comm: String,
    pub state: char,
    pub ppid: i32,
    pub pgid: i32,
    pub sid: i32,
    pub utime: u64,
    pub stime: u64,
    pub vsize: u64,
    pub rss: i64,
}

#[cfg(target_os = "linux")]
impl StatStub {
    pub fn read(pid: i32) -> Result<StatStub, SystemError> {
        let stat_bytes = std::fs::read(format!("/proc/{}/stat", pid))?;
        let stat = String::from_utf8_lossy(&stat_bytes);
        Self::parse_stat(stat.trim())
    }

    #[allow(clippy::field_reassign_with_default)]
    fn parse_stat(stat: &str) -> Result<StatStub, SystemError> {
        #[inline(always)]
        fn next<'l, T: std::str::FromStr, I: Iterator<Item = &'l str>>(
            it: &mut I,
        ) -> Result<T, SystemError> {
            it.next()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| SystemError::Error("proc stat: invalid entry".into()))
        }

        let (cmd_start, cmd_end) = stat
            .find('(')
            .zip(stat.rfind(')'))
            .ok_or_else(|| SystemError::Error("proc stat: tcomm not found".into()))?;

        let mut stub = StatStub::default();
        stub.pid = next(&mut std::iter::once(&stat[..cmd_start - 1]))?;
        stub.comm = stat[cmd_start + 1..cmd_end].to_string();

        let mut it = stat[cmd_end + 2..].split(' ');
        stub.state = next(&mut it)?;
        stub.ppid = next(&mut it)?;
        stub.pgid = next(&mut it)?;
        stub.sid = next(&mut it)?;

        // tty_nr, tpgid, flags, minflt, cminflt, majflt, cmajflt
        let mut it = it.skip(7);
        stub.utime = next(&mut it)?;
        stub.stime = next(&mut it)?;

        // cutime, cstime, priority, nice, num_threads, itrealvalue, starttime
        let mut it = it.skip(7);
        stub.vsize = next(&mut it)?;
        stub.rss = next(&mut it)?;

        Ok(stub)
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
        _ => Err(SystemError::from(nix::Error::last())),
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

#[cfg(test)]
mod test {

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_stat() {
        let stat = "10220 (proc-name) S 7666 7832 7832 0 -1 4194304 2266 0 4 0 44 2 0 0 \
        20 0 6 0 1601 816193536 1793 18446744073709551615 94873375535104 94873375567061 \
        140731153968032 0 0 0 0 4096 0 0 0 0 17 6 0 0 0 0 0 94873375582256 94873375584384 \
        94873398587392 140731153974918 140731153974959 140731153974959 140731153977295 0";

        let parsed = super::StatStub::parse_stat(stat).unwrap();
        let expected = super::StatStub {
            pid: 10220,
            comm: "proc-name".to_string(),
            state: 'S',
            ppid: 7666,
            pgid: 7832,
            sid: 7832,
            utime: 44,
            stime: 2,
            vsize: 816193536,
            rss: 1793,
        };

        assert_eq!(parsed, expected);
    }
}
