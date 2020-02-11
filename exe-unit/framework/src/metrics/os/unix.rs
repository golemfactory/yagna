use crate::metrics::{error::MetricError, Result};
use nix::errno::{errno, Errno};
use nix::libc;
use nix::unistd::{getpid, Pid};
use std::mem;
use std::time::Duration;

pub type SystemError = nix::Error;

pub fn cpu_time() -> Result<Duration> {
    let process = to_duration(&getrusage(Resource::Process)?);
    let children = to_duration(&getrusage(Resource::Children)?);
    Ok(process + children)
}

pub fn mem_rss() -> Result<i64> {
    // Include child processes with:
    //
    // let children = getrusage(Resource::Children)?;
    Err(MetricError::Unsupported)
}

pub fn mem_peak_rss() -> Result<i64> {
    let process = getrusage(Resource::Process)?;
    Ok(process.ru_maxrss + process.ru_ixrss + process.ru_idrss + process.ru_isrss)
}

#[repr(i32)]
enum Resource {
    Process = 0,
    Children = -1,
}

fn getrusage(resource: Resource) -> Result<libc::rusage> {
    let mut usage = mem::MaybeUninit::<libc::rusage>::uninit();
    let ret = unsafe { libc::getrusage(resource as i32, usage.as_mut_ptr()) };
    match ret {
        0 => Ok(unsafe { usage.assume_init() }),
        _ => Err(nix::Error::last().into()),
    }
}

fn to_duration(usage: &libc::rusage) -> Duration {
    let sec = (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as u64;
    let usec = (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as u64;
    Duration::from_secs(sec) + Duration::from_micros(usec)
}
