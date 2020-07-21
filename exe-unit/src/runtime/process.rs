use crate::error::Error;
use crate::message::{ExecuteCommand, SetRuntimeMode, SetTaskPackagePath, Shutdown};
use crate::process::{ProcessTree, SystemError};
use crate::runtime::event::EventMonitor;
use crate::runtime::{Runtime, RuntimeArgs, RuntimeMode};
use crate::ExeUnitContext;
use actix::prelude::*;
use futures::channel::mpsc;
use futures::future::LocalBoxFuture;
use futures::prelude::*;
use futures::{FutureExt, SinkExt, TryFutureExt};
use std::collections::HashSet;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio_util::codec::{BytesCodec, FramedRead};
use ya_client_model::activity::{ExeScriptCommand, RuntimeEvent};
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
            ChildProcess::Tree(tree) => tree.kill(timeout).boxed_local(),
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
        cmd: ExecuteCommand,
        address: Addr<Self>,
    ) -> LocalBoxFuture<'f, Result<i32, Error>> {
        let idx = cmd.idx;
        let evt_tx = cmd.tx.clone();

        let cmd_args = match cmd.command {
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
            _ => return futures::future::ok(0).boxed_local(),
        };
        let binary = self.binary.clone();
        let args = self.args(cmd_args);

        log::info!(
            "Executing {:?} with {:?} from path {:?}",
            binary,
            args,
            std::env::current_dir()
        );

        let batch_id = cmd.batch_id.clone();
        async move {
            let mut child = Command::new(binary)
                .kill_on_drop(true)
                .args(args?)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let id = batch_id.clone();
            forward_output(child.stdout.take().unwrap(), &evt_tx, move |out| {
                RuntimeEvent::stdout(id.clone(), idx, out)
            });
            let id = batch_id.clone();
            forward_output(child.stderr.take().unwrap(), &evt_tx, move |out| {
                RuntimeEvent::stderr(id.clone(), idx, out)
            });

            let tree =
                ProcessTree::try_new(child.id()).map_err(|e| Error::RuntimeError(e.to_string()))?;

            address.do_send(AddChildProcess::from(tree.clone()));
            let result = child.await;
            address.do_send(RemoveChildProcess::from(tree));

            Ok(result?.code().unwrap_or(-1))
        }
        .boxed_local()
    }

    fn handle_service_command<'f>(
        &mut self,
        cmd: ExecuteCommand,
        address: Addr<Self>,
    ) -> LocalBoxFuture<'f, Result<i32, Error>> {
        let binary = self.binary.clone();
        match cmd.command {
            ExeScriptCommand::Start { args } => {
                let monitor = self.monitor.get_or_insert_with(Default::default).clone();

                async move {
                    let mut cmd_args = vec![String::from("start")];
                    cmd_args.extend(args);
                    let mut command = Command::new(binary);
                    command.args(cmd_args);

                    let service = spawn(command, monitor)
                        .map_err(|e| Error::RuntimeError(e.to_string()))
                        .await?;
                    service
                        .hello(SERVICE_PROTOCOL_VERSION)
                        .map_err(|e| Error::RuntimeError(format!("{:?}", e)))
                        .await?;
                    address
                        .send(SetProcessService(ProcessService::new(service)))
                        .await?;
                    Ok(0)
                }
                .boxed_local()
            }
            ExeScriptCommand::Run { entry_point, args } => {
                let mut cmd_args = vec![
                    String::from("run"),
                    String::from("--entrypoint"),
                    String::from(entry_point),
                ];
                cmd_args.extend(args);

                let mut run_process = RunProcess::default();
                run_process.bin = binary.display().to_string();
                run_process.args = cmd_args;

                let service = self.service.as_ref().unwrap().service.clone();
                let mut monitor = self.monitor.as_ref().unwrap().clone();
                let batch_id = cmd.batch_id.clone();
                let idx = cmd.idx;
                let mut tx = cmd.tx.clone();

                async move {
                    let process = match service.run_process(run_process).await {
                        Ok(result) => result,
                        Err(error) => return Err(Error::RuntimeError(format!("{:?}", error))),
                    };
                    let mut events = match monitor.events(process.pid) {
                        Some(events) => events,
                        _ => return Err(Error::RuntimeError("Process already monitored".into())),
                    };
                    while let Some(status) = events.rx.next().await {
                        if let Some(out) = vec_to_string(status.stdout) {
                            let batch_id = batch_id.clone();
                            let _ = tx.send(RuntimeEvent::stdout(batch_id, idx, out)).await;
                        }
                        if let Some(out) = vec_to_string(status.stderr) {
                            let batch_id = batch_id.clone();
                            let _ = tx.send(RuntimeEvent::stderr(batch_id, idx, out)).await;
                        }
                        if !status.running {
                            return Ok(status.return_code);
                        }
                    }
                    Ok(0)
                }
                .boxed_local()
            }
            _ => futures::future::ok(0).boxed_local(),
        }
    }
}

fn forward_output<F, R>(read: R, tx: &mpsc::Sender<RuntimeEvent>, f: F)
where
    F: Fn(String) -> RuntimeEvent + 'static,
    R: tokio::io::AsyncRead + 'static,
{
    let tx = tx.clone();
    let stream = FramedRead::new(read, BytesCodec::new())
        .filter_map(|result| async { result.ok() })
        .filter_map(|bytes| async move { bytes_to_string(bytes) })
        .ready_chunks(16)
        .map(|v| v.join("\n"))
        .map(f)
        .map(|evt| Ok(evt));
    Arbiter::spawn(async move {
        if let Err(e) = stream.forward(tx).await {
            log::error!("Error forwarding output: {:?}", e);
        }
    });
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

impl Handler<ExecuteCommand> for RuntimeProcess {
    type Result = ResponseFuture<<ExecuteCommand as Message>::Result>;

    fn handle(&mut self, cmd: ExecuteCommand, ctx: &mut Self::Context) -> Self::Result {
        let address = ctx.address();
        match &cmd.command {
            ExeScriptCommand::Deploy {} => self.handle_process_command(cmd, address),
            _ => match &self.mode {
                RuntimeMode::ProcessPerCommand => self.handle_process_command(cmd, address),
                RuntimeMode::Service => self.handle_service_command(cmd, address),
            },
        }
        .map(|code| Ok(code.unwrap_or(0)))
        .boxed_local()
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

fn bytes_to_string<B: AsRef<[u8]>>(bytes: B) -> Option<String> {
    let bytes = bytes.as_ref();
    let string = String::from_utf8_lossy(bytes);
    if string.is_empty() {
        return None;
    }
    Some(string.to_string())
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

#[derive(Message)]
#[rtype("()")]
struct RemoveChildProcess(ChildProcess);

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
