use winapi::shared::minwindef::{DWORD, LPVOID};
use winapi::um;
use winapi::um::handleapi::INVALID_HANDLE_VALUE;
use winapi::um::winnt::HANDLE;

use std::{mem, ptr};

#[derive(Clone, Debug)]
pub struct JobObject {
    handle: HANDLE,
}

impl JobObject {
    pub fn new() -> anyhow::Result<Self> {
        let proc_handle = unsafe { um::processthreadsapi::GetCurrentProcess() };
        if proc_handle.is_null() {
            anyhow::bail!("failed to get process handle")
        }
        let job_handle = unsafe { um::jobapi2::CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        if job_handle.is_null() {
            anyhow::bail!("failed to create job")
        }
        let me = JobObject { handle: job_handle };
        if unsafe { um::jobapi2::AssignProcessToJobObject(me.handle, proc_handle) } == 0 {
            anyhow::bail!("failed to assign process to job")
        }

        let mut info: um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = um::winnt::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ret = unsafe {
            um::jobapi2::SetInformationJobObject(
                job_handle,
                um::winnt::JobObjectExtendedLimitInformation,
                &mut info as *mut _ as LPVOID,
                mem::size_of::<um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as DWORD,
            )
        };
        if ret == 0 {
            anyhow::bail!("failed to setup job")
        }
        Ok(me)
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        let handle = mem::replace(&mut self.handle, INVALID_HANDLE_VALUE);
        if handle != INVALID_HANDLE_VALUE && unsafe { um::handleapi::CloseHandle(self.handle) } == 0
        {
            let code = unsafe { um::errhandlingapi::GetLastError() };
            log::error!("winapi: {}", code);
        }
    }
}
