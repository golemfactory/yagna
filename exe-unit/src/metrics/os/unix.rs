use crate::metrics::{error::MetricError, Result};
use nix::libc;
use std::mem;
use std::time::Duration;

pub type SystemError = nix::Error;

pub fn cpu_time() -> Result<Duration> {
    let process = to_duration(&getrusage(Resource::Process)?);
    let children = to_duration(&getrusage(Resource::Children)?);
    Ok(process + children)
}

pub fn mem_rss() -> Result<f64> {
    Err(MetricError::Unsupported("mem".to_owned()))
}

pub fn mem_peak_rss() -> Result<f64> {
    let children = getrusage(Resource::Children)?;
    let process = getrusage(Resource::Process)?;
    let total = process.ru_maxrss
        + process.ru_ixrss
        + process.ru_idrss
        + process.ru_isrss
        + children.ru_maxrss
        + children.ru_ixrss
        + children.ru_idrss
        + children.ru_isrss;
    Ok((total as f64) / (1024_f64 * 1024_f64)) // kiB to giB
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
