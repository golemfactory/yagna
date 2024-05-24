use std::mem;
use std::ptr;

use winapi::shared::minwindef::{DWORD, LPDWORD, LPVOID};
use winapi::shared::ntdef::{HANDLE, NULL};
use winapi::um;
use winapi::um::handleapi::INVALID_HANDLE_VALUE;

use crate::SystemError;

#[derive(Clone, Debug)]
pub struct JobObject {
    handle: HANDLE,
}

unsafe impl Send for JobObject {}

impl TryFrom<HANDLE> for JobObject {
    type Error = SystemError;

    fn try_from(process_handle: HANDLE) -> Result<Self, Self::Error> {
        let handle = Self::create_job(process_handle)?;
        let job_object = JobObject { handle };
        let mut info: um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = um::winnt::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        job_object.set_limits(info).unwrap();
        Ok(job_object)
    }
}

impl JobObject {
    /// Creates new JobObject for the current process.
    pub fn try_new_current() -> Result<Self, SystemError> {
        let handle = current_process_handle();
        Self::try_from(handle)
    }

    pub fn try_new(pid: Option<u32>) -> Result<Self, SystemError> {
        let process_handle = process_handle(pid)?;
        Self::try_from(process_handle)
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
            return Err(SystemError::last());
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
            return Err(SystemError::last());
        }

        Ok(info)
    }

    pub fn terminate(&self) -> Result<(), SystemError> {
        if unsafe { um::jobapi2::TerminateJobObject(self.handle, 0) } == 0 {
            return Err(SystemError::last());
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
            return Err(SystemError::last());
        }
        Ok(())
    }

    fn create_job(proc: HANDLE) -> Result<HANDLE, SystemError> {
        let handle = unsafe { um::jobapi2::CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        if handle.is_null() {
            return Err(SystemError::NullPointer("JobObject handle".to_string()));
        }
        if unsafe { um::jobapi2::AssignProcessToJobObject(handle, proc) } == 0 {
            return Err(SystemError::last());
        }

        Ok(handle)
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        let handle = mem::replace(&mut self.handle, INVALID_HANDLE_VALUE);
        if handle != INVALID_HANDLE_VALUE && unsafe { um::handleapi::CloseHandle(self.handle) } == 0
        {
            log::error!("{:?}", SystemError::last());
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
                return Err(SystemError::NullPointer(format!("process {} handle", pid)));
            }
            Ok(handle)
        }
        _ => {
            let handle = unsafe { um::processthreadsapi::GetCurrentProcess() };
            Ok(handle)
        }
    }
}

fn current_process_handle() -> HANDLE {
    unsafe { um::processthreadsapi::GetCurrentProcess() }
}
