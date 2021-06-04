use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::hash::{Hash, Hasher};
use std::ops::Not;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use actix::prelude::*;
use futures::future::{self, LocalBoxFuture};
use futures::{FutureExt, TryFutureExt};
use tokio::process::Command;

use crate::error::Error;
use crate::message::{ExecuteCommand, SetRuntimeMode, SetTaskPackagePath, Shutdown};
use crate::output::{forward_output, vec_to_string};
use crate::process::{kill, ProcessTree, SystemError};
use crate::runtime::event::EventMonitor;
use crate::runtime::{Runtime, RuntimeMode};
use crate::ExeUnitContext;

use ya_agreement_utils::agreement::OfferTemplate;
use ya_client_model::activity::{CommandOutput, ExeScriptCommand, RuntimeEvent};
use ya_runtime_api::server::{spawn, ProcessControl, RunProcess, RuntimeService, RuntimeStatus};

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
    ctx: ExeUnitContext,
    mode: RuntimeMode,
    binary: PathBuf,
    image_path: Option<PathBuf>,
    children: HashSet<ChildProcess>,
    service: Option<ProcessService>,
    monitor: Option<EventMonitor>,
}

impl RuntimeProcess {
    pub fn new(ctx: &ExeUnitContext, binary: PathBuf) -> Self {
        Self {
            ctx: ctx.clone(),
            mode: Default::default(),
            binary,
            image_path: None,
            children: Default::default(),
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
                Ok(serde_json::from_str(&stdout).map_err(|e| {
                    let msg = format!("Invalid offer template [{}]: {:?}", binary.display(), e);
                    Error::Other(msg)
                })?)
            }
            false => {
                log::info!(
                    "Cannot read offer template from runtime; using defaults [{}]",
                    binary.display()
                );
                Ok(OfferTemplate::default())
            }
        }
    }

    fn args(&self) -> Result<CommandArgs, Error> {
        let mut args = CommandArgs::default();

        args.arg("--workdir");
        args.arg(&self.ctx.work_dir);

        if self.ctx.supervise.image {
            match self.image_path.as_ref() {
                Some(val) => {
                    args.arg("--task-package");
                    args.arg(val.display().to_string());
                }
                None => {
                    return Err(Error::Other(
                        "task package (image) path was not provided".into(),
                    ))
                }
            }
        }

        if !self.ctx.supervise.hardware {
            let inf = &self.ctx.agreement.infrastructure;

            if let Some(val) = inf.get("cpu.threads") {
                args.arg("--cpu-cores");
                args.arg((*val as u64).to_string());
            }
            if let Some(val) = inf.get("mem.gib") {
                args.arg("--mem-gib");
                args.arg(val.to_string());
            }
            if let Some(val) = inf.get("storage.gib").cloned() {
                args.arg("--storage-gib");
                args.arg(val.to_string());
            }
        }

        Ok(args)
    }
}

impl RuntimeProcess {
    fn handle_process_command<'f>(
        &self,
        cmd: ExecuteCommand,
        address: Addr<Self>,
    ) -> LocalBoxFuture<'f, Result<i32, Error>> {
        let mut rt_args = match self.args() {
            Ok(args) => args,
            Err(err) => return futures::future::err(err).boxed_local(),
        };

        let (cmd, ctx) = cmd.split();
        match cmd {
            ExeScriptCommand::Deploy {} => rt_args.args(&["deploy", "--"]),
            ExeScriptCommand::Start { args } => rt_args.args(&["start", "--"]).args(args),
            ExeScriptCommand::Run {
                entry_point, args, ..
            } => rt_args
                .args(&["run", "--entrypoint"])
                .arg(entry_point)
                .arg("--")
                .args(args),
            _ => return future::ok(0).boxed_local(),
        };

        let binary = self.binary.clone();

        log::info!(
            "Executing {:?} with {:?} from path {:?}",
            binary,
            rt_args,
            std::env::current_dir()
        );

        async move {
            let mut child = Command::new(binary)
                .args(rt_args)
                .kill_on_drop(true)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let idx = ctx.idx;
            let id = ctx.batch_id.clone();
            let stdout = forward_output(child.stdout.take().unwrap(), &ctx.tx, move |out| {
                RuntimeEvent::stdout(id.clone(), idx, CommandOutput::Bin(out))
            });
            let id = ctx.batch_id.clone();
            let stderr = forward_output(child.stderr.take().unwrap(), &ctx.tx, move |out| {
                RuntimeEvent::stderr(id.clone(), idx, CommandOutput::Bin(out))
            });

            let proc = if cfg!(feature = "sgx") {
                ChildProcess::from(child.id())
            } else {
                let tree = ProcessTree::try_new(child.id()).map_err(Error::runtime)?;
                ChildProcess::from(tree)
            };
            let _guard = ChildProcessGuard::new(proc, address.clone());

            let result = future::join3(child, stdout, stderr).await;
            Ok(result.0?.code().unwrap_or(-1))
        }
        .boxed_local()
    }

    fn handle_service_command<'f>(
        &mut self,
        cmd: ExecuteCommand,
        address: Addr<Self>,
    ) -> LocalBoxFuture<'f, Result<i32, Error>> {
        let binary = self.binary.clone();
        let (cmd, ctx) = cmd.split();

        match cmd {
            ExeScriptCommand::Start { args } => {
                let mut rt_args = match self.args() {
                    Ok(args) => args,
                    Err(err) => return futures::future::err(err).boxed_local(),
                };
                rt_args.arg("start");
                rt_args.args(args);

                log::info!(
                    "Executing {:?} with {:?} from path {:?}",
                    binary,
                    rt_args,
                    std::env::current_dir()
                );

                let mut monitor = self.monitor.get_or_insert_with(Default::default).clone();
                let mut command = Command::new(binary);
                command.args(rt_args);

                async move {
                    let service = spawn(command, monitor.clone())
                        .map_err(Error::runtime)
                        .await?;
                    let hello = service
                        .hello(SERVICE_PROTOCOL_VERSION)
                        .map_err(|e| Error::runtime(format!("service hello error: {:?}", e)));

                    let _handle = monitor.any_process(ctx);
                    match future::select(service.exited(), hello).await {
                        future::Either::Left((result, _)) => return Ok(result),
                        future::Either::Right((result, _)) => result.map(|_| ())?,
                    }

                    address
                        .send(SetProcessService(ProcessService::new(service)))
                        .await?;
                    Ok(0)
                }
                .boxed_local()
            }
            ExeScriptCommand::Run {
                entry_point,
                mut args,
                ..
            } => {
                let (service, status) = match self.service.as_ref() {
                    Some(svc) => (svc.service.clone(), svc.status.clone()),
                    None => {
                        return future::err(Error::runtime("START command not run")).boxed_local()
                    }
                };

                log::info!("Executing {:?} with {} {:?}", binary, entry_point, args);

                let mut monitor = self.monitor.get_or_insert_with(Default::default).clone();
                let exec = async move {
                    let name = Path::new(&entry_point)
                        .file_name()
                        .ok_or_else(|| Error::runtime("Invalid binary name"))?;
                    args.insert(0, name.to_string_lossy().to_string());

                    let mut run_process = RunProcess::default();
                    run_process.bin = entry_point;
                    run_process.args = args;

                    let process = match service.run_process(run_process).await {
                        Ok(result) => result,
                        Err(error) => return Err(Error::RuntimeError(format!("{:?}", error))),
                    };

                    Ok(monitor.process(ctx, process.pid).await)
                };

                async move {
                    futures::pin_mut!(exec);
                    let exited = status.exited().map(Ok);
                    future::select(exited, exec).await.factor_first().0
                }
                .boxed_local()
            }
            _ => future::ok(0).boxed_local(),
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
    }
}

impl Handler<SetTaskPackagePath> for RuntimeProcess {
    type Result = <SetTaskPackagePath as Message>::Result;

    fn handle(&mut self, msg: SetTaskPackagePath, _: &mut Self::Context) -> Self::Result {
        self.image_path = msg.0;
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
        let add_child = AddChildProcess(ChildProcess::from(msg.0.clone()));
        ctx.address().do_send(add_child);
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
        let service = self.service.take();
        let mut children = std::mem::replace(&mut self.children, HashSet::new());

        async move {
            if let Some(svc) = service {
                let _ = svc.service.shutdown().await;
            }
            let _ = future::join_all(children.drain().map(move |t| t.kill(timeout))).await;
            Ok(())
        }
        .boxed_local()
    }
}

#[derive(Clone, Hash, Eq, PartialEq)]
enum ChildProcess {
    Single { pid: u32 },
    Tree(ProcessTree),
    Service(ProcessService),
}

impl ChildProcess {
    fn kill<'f>(self, timeout: i64) -> LocalBoxFuture<'f, Result<(), SystemError>> {
        match self {
            ChildProcess::Service(service) => async move {
                service.control.kill();
                Ok(())
            }
            .boxed_local(),
            ChildProcess::Tree(tree) => tree.kill(timeout).boxed_local(),
            ChildProcess::Single { pid } => kill(pid as i32, timeout).boxed_local(),
        }
    }
}

impl From<ProcessTree> for ChildProcess {
    fn from(process: ProcessTree) -> Self {
        ChildProcess::Tree(process)
    }
}

impl From<ProcessService> for ChildProcess {
    fn from(service: ProcessService) -> Self {
        ChildProcess::Service(service)
    }
}

impl From<u32> for ChildProcess {
    fn from(pid: u32) -> Self {
        ChildProcess::Single { pid }
    }
}

struct ChildProcessGuard {
    inner: ChildProcess,
    addr: Addr<RuntimeProcess>,
}

impl ChildProcessGuard {
    fn new(inner: ChildProcess, addr: Addr<RuntimeProcess>) -> Self {
        addr.do_send(AddChildProcess(inner.clone()));
        ChildProcessGuard {
            inner,
            addr: addr.clone(),
        }
    }
}

impl Drop for ChildProcessGuard {
    fn drop(&mut self) {
        self.addr.do_send(RemoveChildProcess(self.inner.clone()));
    }
}

#[derive(Clone, Default)]
struct CommandArgs {
    inner: Vec<OsString>,
}

impl CommandArgs {
    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.inner.push(arg.as_ref().to_os_string());
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            self.arg(arg.as_ref());
        }
        self
    }
}

impl IntoIterator for CommandArgs {
    type Item = OsString;
    type IntoIter = std::vec::IntoIter<OsString>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl std::fmt::Debug for CommandArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as std::fmt::Display>::fmt(self, f)
    }
}

impl std::fmt::Display for CommandArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}",
            self.inner.iter().fold(OsString::new(), |mut out, s| {
                out.is_empty().not().then(|| out.push(" "));
                out.push(s);
                out
            })
        )
    }
}

#[derive(Clone)]
struct ProcessService {
    service: Arc<dyn RuntimeService + Send + Sync + 'static>,
    control: Arc<dyn ProcessControl + Send + Sync + 'static>,
    status: Arc<dyn RuntimeStatus + Send + Sync + 'static>,
}

impl ProcessService {
    pub fn new<S>(service: S) -> Self
    where
        S: RuntimeService + RuntimeStatus + ProcessControl + Clone + Send + Sync + 'static,
    {
        ProcessService {
            service: Arc::new(service.clone()),
            control: Arc::new(service.clone()),
            status: Arc::new(service),
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

#[derive(Message)]
#[rtype("()")]
struct SetProcessService(ProcessService);

#[derive(Message)]
#[rtype("()")]
struct AddChildProcess(ChildProcess);

#[derive(Message)]
#[rtype("()")]
struct RemoveChildProcess(ChildProcess);
