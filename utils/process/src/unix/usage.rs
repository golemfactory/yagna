use super::Process;

impl Process {
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
    let ret = unsafe { libc::getrusage(resource, usage.as_mut_ptr()) };
    match ret {
        0 => Ok(Usage::from(unsafe { usage.assume_init() })),
        _ => Err(SystemError::from(nix::Error::last())),
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
