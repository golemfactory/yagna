use crate::metrics::Result;
use std::mem;
use std::ptr;
use std::time::Duration;
use thiserror::Error;
use winapi::shared::minwindef::{DWORD, FALSE, FILETIME};
use winapi::um;

#[derive(Clone, Debug, Error)]
pub enum SystemError {
    #[error("Null pointer: {0}")]
    NullPointer(String),
    #[error("API error: {0}")]
    ApiError(u32),
}

impl SystemError {
    pub fn last() -> Self {
        let code = unsafe { um::errhandlingapi::GetLastError() };
        SystemError::ApiError(code)
    }
}

pub fn cpu_time() -> Result<Duration> {
    let pid = unsafe { um::processthreadsapi::GetCurrentProcessId() };
    let times = proc_times(pid)?;
    Ok(to_duration(&times.kernel_time) + to_duration(&times.user_time))
}

pub fn mem_rss() -> Result<i64> {
    let pid = unsafe { um::processthreadsapi::GetCurrentProcessId() };
    let info = mem_info(pid)?;
    Ok(info.WorkingSetSize as i64)
}

pub fn mem_peak_rss() -> Result<i64> {
    let pid = unsafe { um::processthreadsapi::GetCurrentProcessId() };
    let info = mem_info(pid)?;
    Ok(info.PeakWorkingSetSize as i64)
}

struct ProcessTimes {
    creation_time: FILETIME,
    exit_time: FILETIME,
    kernel_time: FILETIME,
    user_time: FILETIME,
}

fn proc_times(pid: DWORD) -> Result<ProcessTimes> {
    let mut creation_time = mem::MaybeUninit::<FILETIME>::uninit();
    let mut exit_time = mem::MaybeUninit::<FILETIME>::uninit();
    let mut kernel_time = mem::MaybeUninit::<FILETIME>::uninit();
    let mut user_time = mem::MaybeUninit::<FILETIME>::uninit();

    let handle = unsafe {
        um::processthreadsapi::OpenProcess(
            um::winnt::PROCESS_QUERY_INFORMATION | um::winnt::PROCESS_VM_READ,
            FALSE,
            pid,
        )
    };
    if handle.is_null() {
        return Err(SystemError::NullPointer(format!("handle to process {}", pid)).into());
    }

    let ret = unsafe {
        um::processthreadsapi::GetProcessTimes(
            handle,
            creation_time.as_mut_ptr(),
            exit_time.as_mut_ptr(),
            kernel_time.as_mut_ptr(),
            user_time.as_mut_ptr(),
        )
    };
    unsafe { um::handleapi::CloseHandle(handle) };

    match ret {
        0 => Err(SystemError::last().into()),
        _ => Ok(ProcessTimes {
            creation_time: unsafe { creation_time.assume_init() },
            exit_time: unsafe { exit_time.assume_init() },
            kernel_time: unsafe { kernel_time.assume_init() },
            user_time: unsafe { user_time.assume_init() },
        }),
    }
}

fn mem_info(pid: DWORD) -> Result<um::psapi::PROCESS_MEMORY_COUNTERS> {
    let handle = unsafe {
        um::processthreadsapi::OpenProcess(
            um::winnt::PROCESS_QUERY_INFORMATION | um::winnt::PROCESS_VM_READ,
            FALSE,
            pid,
        )
    };
    if handle.is_null() {
        return Err(SystemError::NullPointer(format!("handle to process {}", pid)).into());
    }

    let mut counters = mem::MaybeUninit::<um::psapi::PROCESS_MEMORY_COUNTERS>::uninit();
    let ret = unsafe {
        um::psapi::GetProcessMemoryInfo(
            handle,
            counters.as_mut_ptr(),
            mem::size_of::<um::psapi::PROCESS_MEMORY_COUNTERS>() as DWORD,
        )
    };
    unsafe { um::handleapi::CloseHandle(handle) };

    match ret {
        0 => Err(SystemError::last().into()),
        _ => Ok(unsafe { counters.assume_init() }),
    }
}

fn to_duration(ft: &FILETIME) -> Duration {
    Duration::from_nanos(((ft.dwHighDateTime as u64) << 32) + ft.dwLowDateTime as u64)
}
