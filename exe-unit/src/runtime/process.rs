use crate::error::Error;
use crate::message::{
    RuntimeCommand, RuntimeCommandResult, SetRuntimeMode, SetTaskPackagePath, Shutdown,
};
#[cfg(feature = "sgx")]
use crate::process::kill;
#[cfg(not(feature = "sgx"))]
use crate::process::ProcessTree;
use crate::process::SystemError;
use crate::runtime::event::EventMonitor;
use crate::runtime::{Runtime, RuntimeArgs, RuntimeMode};
use crate::ExeUnitContext;
use actix::prelude::*;
use futures::future::LocalBoxFuture;
use futures::prelude::*;
use futures::FutureExt;
use std::collections::HashSet;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use ya_agreement_utils::agreement::OfferTemplate;
use ya_client_model::activity::{CommandResult, ExeScriptCommand};
use ya_runtime_api::server::{spawn, ProcessControl, RunProcess, RuntimeService};

const PROCESS_KILL_TIMEOUT_SECONDS_ENV_VAR: &str = "PROCESS_KILL_TIMEOUT_SECONDS";
const DEFAULT_PROCESS_KILL_TIMEOUT_SECONDS: i64 = 5;
const MIN_PROCESS_KILL_TIMEOUT_SECONDS: i64 = 1;
const SERVICE_PROTOCOL_VERSION: &str = "0.1.0";

fn process_kill_timeout_seconds() -> i64 {
    let limit = std::env::var(PROCESS_KILL_TIMEOUT_SECONDS_ENV_VAR)
        .and_then(|v| v.parse().map_err(|_| std::env::VarError::NotPresent))
        .unwrap_or(DEFAULT_PROCESS_KILL_TIMEOUT_SECONDS);
    std::cmp::max(limit, MIN_PROCESS_KILL_TIMEOUT_SECONDS)
}

pub struct RuntimeProcess {
    binary: PathBuf,
    runtime_args: RuntimeArgs,
    task_package_path: Option<PathBuf>,
    mode: RuntimeMode,
    children: HashSet<ChildProcess>,
    service: Option<ProcessService>,
    monitor: Option<EventMonitor>,
}

#[derive(Clone, Hash, Eq, PartialEq)]
enum ChildProcess {
    #[cfg(feature = "sgx")]
    Single {
        pid: u32,
    },
    #[cfg(not(feature = "sgx"))]
    Tree(ProcessTree),
    Service(ProcessService),
}

impl ChildProcess {
    pub fn kill<'f>(self, timeout: i64) -> LocalBoxFuture<'f, Result<(), SystemError>> {
        match self {
            ChildProcess::Service(service) => async move {
                service.control.kill();
                Ok(())
            }
            .boxed_local(),
            #[cfg(not(feature = "sgx"))]
            ChildProcess::Tree(tree) => tree.kill(timeout).boxed_local(),
            #[cfg(feature = "sgx")]
            ChildProcess::Single { pid } => kill(pid as i32, timeout).boxed_local(),
        }
    }
}

#[derive(Clone)]
struct ProcessService {
    service: Arc<dyn RuntimeService + Send + Sync + 'static>,
    control: Arc<dyn ProcessControl + Send + Sync + 'static>,
}

impl ProcessService {
    pub fn new<S>(service: S) -> Self
    where
        S: RuntimeService + ProcessControl + Clone + Send + Sync + 'static,
    {
        ProcessService {
            service: Arc::new(service.clone()),
            control: Arc::new(service),
        }
    }
}

impl Hash for ProcessService {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.control.id())
    }
}

impl PartialEq for ProcessService {
    fn eq(&self, other: &Self) -> bool {
        self.control.id() == other.control.id()
    }
}

impl Eq for ProcessService {}

impl RuntimeProcess {
    pub fn new(ctx: &ExeUnitContext, binary: PathBuf) -> Self {
        Self {
            binary,
            runtime_args: ctx.runtime_args.clone(),
            task_package_path: None,
            mode: RuntimeMode::default(),
            children: HashSet::new(),
            service: None,
            monitor: None,
        }
    }

    pub fn offer_template(binary: PathBuf) -> Result<OfferTemplate, Error> {
        let current_path = std::env::current_dir();
        let args = vec![OsString::from("offer-template")];

        log::info!(
            "Executing {:?} with {:?} from path {:?}",
            binary,
            args,
            current_path
        );

        let child = std::process::Command::new(binary.clone())
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let result = child.wait_with_output()?;
        match result.status.success() {
            true => {
                let stdout = vec_to_string(result.stdout).unwrap_or_else(String::new);
                Ok(serde_json::from_str(&stdout)?)
            }
            false => {
                log::warn!(
                    "Cannot read offer template from runtime; using defaults [{}]",
                    binary.display()
                );
                Ok(OfferTemplate::default())
            }
        }
    }

    fn args(&self, cmd_args: Vec<OsString>) -> Result<Vec<OsString>, Error> {
        let pkg_path = self
            .task_package_path
            .clone()
            .ok_or(Error::RuntimeError("Missing task package path".to_owned()))?;

        let mut args = self.runtime_args.to_command_line(&pkg_path);
        args.extend(cmd_args);
        Ok(args)
    }
}

impl RuntimeProcess {
    fn handle_process_command<'f>(
        &self,
        command: ExeScriptCommand,
        address: Addr<Self>,
    ) -> LocalBoxFuture<'f, Result<RuntimeCommandResult, Error>> {
        let cmd_args = match command {
            ExeScriptCommand::Deploy {} => {
                let cmd_args = vec![OsString::from("deploy")];
                cmd_args
            }
            ExeScriptCommand::Start { args } => {
                let mut cmd_args = vec![OsString::from("start")];
                cmd_args.extend(args.into_iter().map(OsString::from));
                cmd_args
            }
            ExeScriptCommand::Run { entry_point, args } => {
                let mut cmd_args = vec![
                    OsString::from("run"),
                    OsString::from("--entrypoint"),
                    OsString::from(entry_point),
                ];
                cmd_args.extend(args.into_iter().map(OsString::from));
                cmd_args
            }
            _ => return futures::future::ok(RuntimeCommandResult::ok()).boxed_local(),
        };

        let binary = self.binary.clone();
        let args = self.args(cmd_args);
        let current_path = std::env::current_dir();
        log::info!(
            "Executing {:?} with {:?} from path {:?}",
            binary,
            args,
            current_path
        );

        async move {
            let child = Command::new(binary)
                .kill_on_drop(true)
                .args(args?)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            #[cfg(not(feature = "sgx"))]
            let result = {
                let tree = ProcessTree::try_new(child.id())
                    .map_err(|e| Error::RuntimeError(e.to_string()))?;
                address.do_send(AddChildProcess::from(tree.clone()));
                let result = child.wait_with_output().await;
                address.do_send(RemoveChildProcess::from(tree));
                result
            };
            #[cfg(feature = "sgx")]
            let result = {
                let single_child = child.id();
                address.do_send(AddChildProcess::from(single_child));
                let result = child.wait_with_output().await;
                address.do_send(RemoveChildProcess::from(single_child));
                result
            };

            let output = result?;

            Ok(RuntimeCommandResult {
                result: match output.status.success() {
                    true => CommandResult::Ok,
                    _ => CommandResult::Error,
                },
                stdout: vec_to_string(output.stdout),
                stderr: vec_to_string(output.stderr),
            })
        }
        .boxed_local()
    }

    fn handle_service_command<'f>(
        &mut self,
        command: ExeScriptCommand,
        addr: Addr<Self>,
    ) -> LocalBoxFuture<'f, Result<RuntimeCommandResult, Error>> {
        let binary = self.binary.clone();
        match command {
            ExeScriptCommand::Start { args } => {
                let monitor = self.monitor.get_or_insert_with(Default::default).clone();
                let mut cmd_args = vec![OsString::from("start")];
                cmd_args.extend(args.into_iter().map(OsString::from));
                let args = self.args(cmd_args).unwrap_or_else(|_| Vec::new());

                log::info!("Executing {:?} with {:?}", binary, args);

                let mut command = Command::new(binary);
                command.args(args);

                async move {
                    let service = spawn(command, monitor)
                        .map_err(|e| Error::RuntimeError(e.to_string()))
                        .await?;
                    service
                        .hello(SERVICE_PROTOCOL_VERSION)
                        .map_err(|e| Error::RuntimeError(format!("{:?}", e)))
                        .await?;
                    addr.send(SetProcessService(ProcessService::new(service)))
                        .await?;
                    Ok(RuntimeCommandResult::ok())
                }
                .boxed_local()
            }
            ExeScriptCommand::Run {
                entry_point,
                mut args,
            } => {
                log::info!("Executing {:?} with {} {:?}", binary, entry_point, args);

                let service = self.service.as_ref().unwrap().service.clone();
                let mut monitor = self.monitor.as_ref().unwrap().clone();

                async move {
                    let name = Path::new(&entry_point)
                        .file_name()
                        .ok_or_else(|| Error::RuntimeError("Invalid binary name".into()))?;
                    args.insert(0, name.to_string_lossy().to_string());

                    let mut run_process = RunProcess::default();
                    run_process.bin = entry_point;
                    run_process.args = args;

                    let process = match service.run_process(run_process).await {
                        Ok(result) => result,
                        Err(error) => return Err(Error::RuntimeError(format!("{:?}", error))),
                    };
                    let mut events = match monitor.events(process.pid) {
                        Some(events) => events,
                        _ => return Err(Error::RuntimeError("Process handled elsewhere".into())),
                    };

                    let mut stdout = Vec::<u8>::new();
                    let mut stderr = Vec::<u8>::new();
                    let result = loop {
                        let status = match events.rx.next().await {
                            Some(status) => status,
                            _ => continue,
                        };

                        stdout.extend(status.stdout);
                        stderr.extend(status.stderr);
                        if status.running {
                            continue;
                        }

                        break RuntimeCommandResult {
                            result: match status.return_code {
                                0 => CommandResult::Ok,
                                _ => CommandResult::Error,
                            },
                            stdout: vec_to_string(stdout),
                            stderr: vec_to_string(stderr),
                        };
                    };
                    Ok(result)
                }
                .boxed_local()
            }
            _ => futures::future::ok(RuntimeCommandResult::ok()).boxed_local(),
        }
    }
}

impl Runtime for RuntimeProcess {}

impl Actor for RuntimeProcess {
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        log::info!("Runtime handler started");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::info!("Runtime handler stopped");
    }
}

impl Handler<RuntimeCommand> for RuntimeProcess {
    type Result = ResponseFuture<<RuntimeCommand as Message>::Result>;

    fn handle(&mut self, msg: RuntimeCommand, ctx: &mut Self::Context) -> Self::Result {
        let address = ctx.address();
        match &msg.0 {
            ExeScriptCommand::Deploy {} => self.handle_process_command(msg.0, address),
            _ => match &self.mode {
                RuntimeMode::ProcessPerCommand => self.handle_process_command(msg.0, address),
                RuntimeMode::Service => self.handle_service_command(msg.0, address),
            },
        }
    }
}

impl Handler<SetTaskPackagePath> for RuntimeProcess {
    type Result = <SetTaskPackagePath as Message>::Result;

    fn handle(&mut self, msg: SetTaskPackagePath, _: &mut Self::Context) -> Self::Result {
        self.task_package_path = Some(msg.0);
    }
}

impl Handler<SetRuntimeMode> for RuntimeProcess {
    type Result = <SetRuntimeMode as Message>::Result;

    fn handle(&mut self, msg: SetRuntimeMode, _: &mut Self::Context) -> Self::Result {
        log::info!("Setting runtime mode to: {:?}", msg.0);
        self.mode = msg.0;
        Ok(())
    }
}

impl Handler<SetProcessService> for RuntimeProcess {
    type Result = <SetProcessService as Message>::Result;

    fn handle(&mut self, msg: SetProcessService, ctx: &mut Self::Context) -> Self::Result {
        ctx.address().do_send(AddChildProcess::from(msg.0.clone()));
        self.service = Some(msg.0);
    }
}

impl Handler<AddChildProcess> for RuntimeProcess {
    type Result = <AddChildProcess as Message>::Result;

    fn handle(&mut self, msg: AddChildProcess, _: &mut Self::Context) -> Self::Result {
        self.children.insert(msg.0);
    }
}

impl Handler<RemoveChildProcess> for RuntimeProcess {
    type Result = <RemoveChildProcess as Message>::Result;

    fn handle(&mut self, msg: RemoveChildProcess, _: &mut Self::Context) -> Self::Result {
        self.children.remove(&msg.0);
    }
}

impl Handler<Shutdown> for RuntimeProcess {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, _: Shutdown, _: &mut Self::Context) -> Self::Result {
        let timeout = process_kill_timeout_seconds();
        let futs = self.children.drain().map(move |t| t.kill(timeout));

        self.mode = RuntimeMode::default();
        self.service.take();

        futures::future::join_all(futs)
            .map(|_| Ok(()))
            .boxed_local()
    }
}

fn vec_to_string(vec: Vec<u8>) -> Option<String> {
    if vec.is_empty() {
        return None;
    }
    let string = match String::from_utf8(vec) {
        Ok(utf8) => utf8.to_owned(),
        Err(error) => error
            .as_bytes()
            .into_iter()
            .map(|&c| c as char)
            .collect::<String>(),
    };
    Some(string)
}

#[derive(Message)]
#[rtype("()")]
struct SetProcessService(ProcessService);

#[derive(Message)]
#[rtype("()")]
struct AddChildProcess(ChildProcess);

#[cfg(not(feature = "sgx"))]
impl From<ProcessTree> for AddChildProcess {
    fn from(process: ProcessTree) -> Self {
        AddChildProcess(ChildProcess::Tree(process))
    }
}

impl From<ProcessService> for AddChildProcess {
    fn from(service: ProcessService) -> Self {
        AddChildProcess(ChildProcess::Service(service))
    }
}

#[cfg(feature = "sgx")]
impl From<u32> for AddChildProcess {
    fn from(pid: u32) -> Self {
        AddChildProcess(ChildProcess::Single { pid })
    }
}

#[derive(Message)]
#[rtype("()")]
struct RemoveChildProcess(ChildProcess);

#[cfg(not(feature = "sgx"))]
impl From<ProcessTree> for RemoveChildProcess {
    fn from(process: ProcessTree) -> Self {
        RemoveChildProcess(ChildProcess::Tree(process))
    }
}

impl From<ProcessService> for RemoveChildProcess {
    fn from(service: ProcessService) -> Self {
        RemoveChildProcess(ChildProcess::Service(service))
    }
}

#[cfg(feature = "sgx")]
impl From<u32> for RemoveChildProcess {
    fn from(pid: u32) -> Self {
        RemoveChildProcess(ChildProcess::Single { pid })
    }
}
