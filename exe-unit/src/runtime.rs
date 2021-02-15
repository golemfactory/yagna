use crate::agreement::Agreement;
use crate::message::*;
use actix::prelude::*;
use std::ffi::OsString;
use std::path::PathBuf;
use ya_runtime_api::deploy::StartMode;

mod event;
pub mod process;

pub trait Runtime:
    Actor<Context = Context<Self>>
    + Handler<Shutdown>
    + Handler<ExecuteCommand>
    + Handler<UpdateDeployment>
{
}

#[derive(Clone, Debug)]
pub enum RuntimeMode {
    ProcessPerCommand,
    Service,
}

impl Default for RuntimeMode {
    fn default() -> Self {
        RuntimeMode::ProcessPerCommand
    }
}

impl From<StartMode> for RuntimeMode {
    fn from(mode: StartMode) -> Self {
        match mode {
            StartMode::Empty => RuntimeMode::ProcessPerCommand,
            StartMode::Blocking => RuntimeMode::Service,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeArgs {
    workdir: PathBuf,
    task_package: Option<PathBuf>,
    cpu_cores: Option<f64>,
    mem_gib: Option<f64>,
    storage_gib: Option<f64>,
}

impl RuntimeArgs {
    pub fn new(work_dir: &PathBuf, agreement: &Agreement, with_inf: bool) -> Self {
        let mut cpu_cores = None;
        let mut mem_gib = None;
        let mut storage_gib = None;
        if with_inf {
            cpu_cores = agreement.infrastructure.get("cpu.threads").cloned();
            mem_gib = agreement.infrastructure.get("mem.gib").cloned();
            storage_gib = agreement.infrastructure.get("storage.gib").cloned();
        }

        RuntimeArgs {
            workdir: work_dir.clone(),
            task_package: None,
            cpu_cores,
            mem_gib,
            storage_gib,
        }
    }

    pub fn to_command_line(&self, package_path: &PathBuf) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("--workdir"),
            self.workdir.clone().into_os_string(),
            OsString::from("--task-package"),
            package_path.clone().into_os_string(),
        ];
        if let Some(val) = self.cpu_cores {
            args.extend(vec![
                OsString::from("--cpu-cores"),
                OsString::from((val as u64).to_string()),
            ]);
        }
        if let Some(val) = self.mem_gib {
            args.extend(vec![
                OsString::from("--mem-gib"),
                OsString::from(val.to_string()),
            ]);
        }
        if let Some(val) = self.storage_gib {
            args.extend(vec![
                OsString::from("--storage-gib"),
                OsString::from(val.to_string()),
            ]);
        }
        args
    }
}
