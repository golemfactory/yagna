mod job_object;

pub use job_object::*;

use std::hash::Hash;
use std::sync::{Arc, Mutex};

use thiserror::Error;
use winapi::um;

use crate::ProcessError;

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

impl From<SystemError> for ProcessError {
    fn from(err: SystemError) -> Self {
        ProcessError::Other(err.to_string())
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
