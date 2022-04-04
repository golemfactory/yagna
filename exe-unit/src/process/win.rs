use std::hash::Hash;
use std::mem;
use std::ptr;
use std::sync::{Arc, Mutex};

use thiserror::Error;
use winapi::shared::minwindef::{DWORD, LPDWORD, LPVOID};
use winapi::shared::ntdef::{HANDLE, NULL};
use winapi::um;
use winapi::um::handleapi::INVALID_HANDLE_VALUE;

lazy_static::lazy_static! {
    static ref JOB_OBJECT: Arc<Mutex<JobObject>> = {
        let job = JobObject::try_new(None).unwrap();
        Arc::new(Mutex::new(job))
    };
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

impl<T> From<std::sync::PoisonError<T>> for SystemError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        SystemError::PoisonError
    }
}

impl SystemError {
    pub fn last() -> Self {
        let code = unsafe { um::errhandlingapi::GetLastError() };
        SystemError::ApiError(code)
    }
}

#[derive(Clone, Debug)]
pub struct ProcessTree {
    pub pid: u32,
    job: JobObject,
}

unsafe impl Send for ProcessTree {}

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
    #[inline]
    pub fn job() -> Arc<Mutex<JobObject>> {
        (*JOB_OBJECT).clone()
    }

    pub fn try_new(pid: u32) -> Result<Self, SystemError> {
        let job = JobObject::try_new(Some(pid))?;
        Ok(ProcessTree { pid, job })
    }

    pub async fn kill(self, _timeout: i64) -> Result<(), SystemError> {
        self.job.terminate()?;
        Ok(())
    }
}

pub async fn kill(pid: i32, _timeout: i64) -> Result<(), SystemError> {
    let job = JobObject::try_new(Some(pid as u32))?;
    job.terminate()?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct JobObject {
    handle: HANDLE,
}

unsafe impl Send for JobObject {}

impl JobObject {
    pub fn try_new(pid: Option<u32>) -> Result<Self, SystemError> {
        let job_object = JobObject {
            handle: Self::create_job(process_handle(pid)?)?,
        };

        let mut info: um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = um::winnt::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        job_object.set_limits(info).unwrap();

        Ok(job_object)
    }

    pub fn accounting(
        &self,
    ) -> Result<um::winnt::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, SystemError> {
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

    pub fn limits(&self) -> Result<um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION, SystemError> {
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

    pub fn terminate(&self) -> Result<(), SystemError> {
        if unsafe { um::jobapi2::TerminateJobObject(self.handle, 0) } == 0 {
            return Err(SystemError::last().into());
        }
        Ok(())
    }

    fn set_limits(
        &self,
        mut info: um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    ) -> Result<(), SystemError> {
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

    fn create_job(proc: HANDLE) -> Result<HANDLE, SystemError> {
        let handle = unsafe { um::jobapi2::CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        if handle.is_null() {
            return Err(SystemError::NullPointer(format!("JobObject handle")).into());
        }
        if unsafe { um::jobapi2::AssignProcessToJobObject(handle, proc) } == 0 {
            return Err(SystemError::last().into());
        }

        Ok(handle)
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        let handle = mem::replace(&mut self.handle, INVALID_HANDLE_VALUE);
        if handle != INVALID_HANDLE_VALUE {
            if unsafe { um::handleapi::CloseHandle(self.handle) } == 0 {
                log::error!("{:?}", SystemError::last());
            }
        }
    }
}

fn process_handle(pid: Option<u32>) -> Result<HANDLE, SystemError> {
    match pid {
        Some(pid) => {
            let handle = unsafe {
                um::processthreadsapi::OpenProcess(
                    um::winnt::PROCESS_TERMINATE
                        | um::winnt::PROCESS_QUERY_INFORMATION
                        | um::winnt::PROCESS_QUERY_LIMITED_INFORMATION
                        | um::winnt::PROCESS_SET_INFORMATION
                        | um::winnt::PROCESS_SET_LIMITED_INFORMATION
                        | um::winnt::PROCESS_SET_QUOTA,
                    0,
                    pid,
                )
            };
            if handle.is_null() {
                return Err(SystemError::NullPointer(format!("process {} handle", pid)).into());
            }
            Ok(handle)
        }
        _ => {
            let handle = unsafe { um::processthreadsapi::GetCurrentProcess() };
            Ok(handle)
        }
    }
}
