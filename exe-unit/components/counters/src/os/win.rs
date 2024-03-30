use crate::os::process::*;

use crate::error::MetricError;
use crate::Result;
use std::time::Duration;

pub fn cpu_time() -> Result<Duration> {
    let info = ProcessTree::job()
        .lock()
        .map_err(SystemError::from)?
        .accounting()?;
    let user_time = to_duration(unsafe { info.TotalUserTime.u() });
    let kernel_time = to_duration(unsafe { info.TotalKernelTime.u() });

    Ok(user_time + kernel_time)
}

#[inline(always)]
pub fn mem_rss() -> Result<f64> {
    Err(MetricError::Unsupported("mem".to_owned()))
}

pub fn mem_peak_rss() -> Result<f64> {
    let info = ProcessTree::job()
        .lock()
        .map_err(SystemError::from)?
        .limits()?;
    Ok((info.PeakJobMemoryUsed as f64) / (1024_f64 * 1024_f64)) // kiB to giB
}

#[inline(always)]
fn to_duration(large_int: &winapi::shared::ntdef::LARGE_INTEGER_u) -> Duration {
    Duration::from_nanos(((large_int.HighPart as u64) << 32) + large_int.LowPart as u64)
}
