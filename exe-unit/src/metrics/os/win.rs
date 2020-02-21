use crate::metrics::error::MetricError;
use crate::metrics::Result;
use std::mem;
use std::ptr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use thiserror::Error;
use winapi::shared::minwindef::{DWORD, LPDWORD, LPVOID};
use winapi::shared::ntdef::{HANDLE, NULL};
use winapi::um;

lazy_static::lazy_static! {
    static ref JOB_OBJECT: Arc<Mutex<JobObject>> = Arc::new(Mutex::new(JobObject::new()));
}

#[derive(Clone, Debug, Error)]
pub enum SystemError {
    #[error("Null pointer: {0}")]
    NullPointer(String),
    #[error("Mutex poison error")]
    PoisonError,
    #[error("API error: {0}")]
    ApiError(u32),
}

impl<T> From<PoisonError<T>> for SystemError {
    fn from(_: PoisonError<T>) -> Self {
        SystemError::PoisonError
    }
}

impl SystemError {
    pub fn last() -> Self {
        let code = unsafe { um::errhandlingapi::GetLastError() };
        SystemError::ApiError(code)
    }
}

pub fn cpu_time() -> Result<Duration> {
    let info = JOB_OBJECT.lock().map_err(SystemError::from)?.accounting()?;
    let user_time = to_duration(unsafe { info.TotalUserTime.u() });
    let kernel_time = to_duration(unsafe { info.TotalKernelTime.u() });

    Ok(user_time + kernel_time)
}

pub fn mem_rss() -> Result<i64> {
    Err(MetricError::Unsupported)
}

pub fn mem_peak_rss() -> Result<i64> {
    let info = JOB_OBJECT.lock().map_err(SystemError::from)?.limits()?;
    Ok(info.PeakJobMemoryUsed as i64)
}

struct JobObject {
    handle: HANDLE,
}

unsafe impl Send for JobObject {}

impl JobObject {
    pub fn new() -> Self {
        let job_object = JobObject {
            handle: Self::create_job().unwrap(),
        };

        let mut info: um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = um::winnt::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        job_object.set_limits(info).unwrap();

        job_object
    }

    fn accounting(&self) -> Result<um::winnt::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION> {
        let mut info: um::winnt::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { mem::zeroed() };

        if unsafe {
            um::jobapi2::QueryInformationJobObject(
                self.handle,
                um::winnt::JobObjectBasicAccountingInformation,
                &mut info as *mut _ as LPVOID,
                mem::size_of::<um::winnt::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as DWORD,
                NULL as *mut _ as LPDWORD,
            )
        } == 0
        {
            return Err(SystemError::last().into());
        }

        Ok(info)
    }

    fn limits(&self) -> Result<um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION> {
        let mut info: um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };

        if unsafe {
            um::jobapi2::QueryInformationJobObject(
                self.handle,
                um::winnt::JobObjectExtendedLimitInformation,
                &mut info as *mut _ as LPVOID,
                mem::size_of::<um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as DWORD,
                NULL as *mut _ as LPDWORD,
            )
        } == 0
        {
            return Err(SystemError::last().into());
        }

        Ok(info)
    }

    fn set_limits(&self, mut info: um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION) -> Result<()> {
        if unsafe {
            um::jobapi2::SetInformationJobObject(
                self.handle,
                um::winnt::JobObjectExtendedLimitInformation,
                &mut info as *mut _ as LPVOID,
                mem::size_of::<um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as DWORD,
            )
        } == 0
        {
            return Err(SystemError::last().into());
        }
        Ok(())
    }

    fn create_job() -> Result<HANDLE> {
        let handle = unsafe { um::jobapi2::CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        if handle.is_null() {
            return Err(SystemError::NullPointer(format!("handle to JobObject")).into());
        }

        let proc = unsafe { um::processthreadsapi::GetCurrentProcess() };
        if unsafe { um::jobapi2::AssignProcessToJobObject(handle, proc) } == 0 {
            return Err(SystemError::last().into());
        }

        Ok(handle)
    }
}

fn to_duration(large_int: &um::winnt::LARGE_INTEGER_u) -> Duration {
    Duration::from_nanos(((large_int.HighPart as u64) << 32) + large_int.LowPart as u64)
}
