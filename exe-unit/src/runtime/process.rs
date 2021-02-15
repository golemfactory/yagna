use crate::error::{Error, VpnError};
use crate::message::{ExecuteCommand, Shutdown, ShutdownReason, UpdateDeployment};
use crate::network::{gateway, Vpn, VpnEndpoint};
use crate::output::{forward_output, vec_to_string};
use crate::process::{kill, ProcessTree, SystemError};
use crate::runtime::event::EventMonitor;
use crate::runtime::{Runtime, RuntimeArgs, RuntimeMode};
use crate::state::Deployment;
use crate::ExeUnitContext;
use actix::prelude::*;
use futures::future::{self, LocalBoxFuture};
use futures::prelude::*;
use futures::{FutureExt, SinkExt, TryFutureExt};
use std::collections::HashSet;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use ya_agreement_utils::agreement::OfferTemplate;
use ya_client_model::activity::{CommandOutput, ExeScriptCommand, RuntimeEvent};
use ya_runtime_api::{
    server::{
        spawn, CreateNetwork, Network, ProcessControl, RunProcess, RuntimeService, RuntimeStatus,
    },
    PROTOCOL_VERSION,
};

const PROCESS_KILL_TIMEOUT_SECONDS_ENV_VAR: &str = "PROCESS_KILL_TIMEOUT_SECONDS";
const DEFAULT_PROCESS_KILL_TIMEOUT_SECONDS: i64 = 5;
const MIN_PROCESS_KILL_TIMEOUT_SECONDS: i64 = 1;

fn process_kill_timeout_seconds() -> i64 {
    let limit = std::env::var(PROCESS_KILL_TIMEOUT_SECONDS_ENV_VAR)
        .and_then(|v| v.parse().map_err(|_| std::env::VarError::NotPresent))
        .unwrap_or(DEFAULT_PROCESS_KILL_TIMEOUT_SECONDS);
    std::cmp::max(limit, MIN_PROCESS_KILL_TIMEOUT_SECONDS)
}

pub struct RuntimeProcess {
    activity_id: Option<String>,
    binary: PathBuf,
    runtime_args: RuntimeArgs,
    deployment: Deployment,
    children: HashSet<ChildProcess>,
    service: Option<ProcessService>,
    monitor: Option<EventMonitor>,
    vpn: Option<Addr<Vpn>>,
}

impl RuntimeProcess {
    pub fn new(ctx: &ExeUnitContext, binary: PathBuf) -> Self {
        Self {
            activity_id: ctx.activity_id.clone(),
            binary,
            runtime_args: ctx.runtime_args.clone(),
            deployment: Default::default(),
            children: HashSet::new(),
            service: None,
            monitor: None,
            vpn: None,
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
            .deployment
            .task_package
            .clone()
            .ok_or(Error::runtime("missing task package path"))?;

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
        let cmd_args = match &cmd.command {
            ExeScriptCommand::Deploy { .. } => vec![OsString::from("deploy")],
            ExeScriptCommand::Start { args } => ["start", "--"]
                .iter()
                .map(OsString::from)
                .chain(args.into_iter().map(OsString::from))
                .collect(),
            ExeScriptCommand::Run {
                entry_point, args, ..
            } => ["run", "--entrypoint", entry_point.as_str(), "--"]
                .iter()
                .map(OsString::from)
                .chain(args.into_iter().map(OsString::from))
                .collect(),
            _ => return future::ok(0).boxed_local(),
        };

        let binary = self.binary.clone();
        let args = self.args(cmd_args);

        log::info!(
            "Executing {:?} with {:?} from path {:?}",
            binary,
            args,
            std::env::current_dir()
        );

        async move {
            let idx = cmd.idx;
            let mut child = Command::new(binary)
                .kill_on_drop(true)
                .args(args?)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let id = cmd.batch_id.clone();
            let stdout = forward_output(child.stdout.take().unwrap(), &cmd.tx, move |out| {
                RuntimeEvent::stdout(id.clone(), idx, CommandOutput::Bin(out))
            });
            let id = cmd.batch_id.clone();
            let stderr = forward_output(child.stderr.take().unwrap(), &cmd.tx, move |out| {
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
        match cmd.command.clone() {
            ExeScriptCommand::Start { args } => self.handle_service_start(args, address),
            ExeScriptCommand::Run {
                entry_point, args, ..
            } => self.handle_service_run(cmd, entry_point, args),
            _ => future::ok(0).boxed_local(),
        }
    }

    fn handle_service_start<'f>(
        &mut self,
        args: Vec<String>,
        address: Addr<Self>,
    ) -> LocalBoxFuture<'f, Result<i32, Error>> {
        let args = self.args(
            std::iter::once(OsString::from("start"))
                .chain(args.into_iter().map(OsString::from))
                .collect(),
        );

        log::info!("Executing {:?} with {:?}", self.binary, args);

        let activity_id = self.activity_id.clone();
        let deployment = self.deployment.clone();
        let monitor = self.monitor.get_or_insert_with(Default::default).clone();
        let mut command = Command::new(self.binary.clone());

        async move {
            command.args(args?);
            let service = spawn(command, monitor).map_err(Error::runtime).await?;
            let hello = service
                .hello(PROTOCOL_VERSION)
                .map_err(|e| Error::runtime(format!("service hello error: {:?}", e)));

            match future::select(service.exited(), hello).await {
                future::Either::Left((result, _)) => return Ok(result),
                future::Either::Right((result, _)) => result.map(|_| ())?,
            }

            let service_ = service.clone();
            let vpn = async {
                if let Some(vpn) = start_vpn(&service_, activity_id, deployment).await? {
                    address.send(SetVpnService(vpn)).await?;
                }
                Ok::<_, Error>(())
            };

            futures::pin_mut!(vpn);
            match future::select(service.exited(), vpn).await {
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

    fn handle_service_run<'f>(
        &mut self,
        cmd: ExecuteCommand,
        entry_point: String,
        mut args: Vec<String>,
    ) -> LocalBoxFuture<'f, Result<i32, Error>> {
        let (service, status) = match self.service.as_ref() {
            Some(svc) => (svc.service.clone(), svc.status.clone()),
            None => return future::err(Error::runtime("START command not run")).boxed_local(),
        };

        log::info!(
            "Executing {} with {} {:?}",
            self.binary.display(),
            entry_point,
            args
        );

        let mut monitor = self.monitor.get_or_insert_with(Default::default).clone();
        let mut tx = cmd.tx.clone();

        let exec = async move {
            args.insert(
                0,
                Path::new(&entry_point)
                    .file_name()
                    .ok_or_else(|| Error::runtime("Invalid binary name"))?
                    .to_string_lossy()
                    .to_string(),
            );

            let mut run_process = RunProcess::default();
            run_process.bin = entry_point;
            run_process.args = args;

            let process = match service.run_process(run_process).await {
                Ok(result) => result,
                Err(error) => return Err(Error::RuntimeError(format!("{:?}", error))),
            };
            let mut events = match monitor.events(process.pid) {
                Some(events) => events,
                _ => return Err(Error::runtime("Process already monitored")),
            };

            while let Some(status) = events.rx.next().await {
                if !status.stdout.is_empty() {
                    let out = CommandOutput::Bin(status.stdout);
                    let evt = RuntimeEvent::stdout(cmd.batch_id.clone(), cmd.idx, out);
                    let _ = tx.send(evt).await;
                }
                if !status.stderr.is_empty() {
                    let out = CommandOutput::Bin(status.stderr);
                    let evt = RuntimeEvent::stderr(cmd.batch_id.clone(), cmd.idx, out);
                    let _ = tx.send(evt).await;
                }
                if !status.running {
                    return Ok(status.return_code);
                }
            }
            Ok(0)
        };

        async move {
            futures::pin_mut!(exec);
            let exited = status.exited().map(Ok);
            future::select(exited, exec).await.factor_first().0
        }
        .boxed_local()
    }
}

async fn start_vpn<R: RuntimeService>(
    service: &R,
    activity_id: Option<String>,
    deployment: Deployment,
) -> Result<Option<Addr<Vpn>>, Error> {
    if activity_id.is_none() || !deployment.networking() {
        return Ok(None);
    }

    let activity_id = activity_id.unwrap();
    let response = service
        .create_network(CreateNetwork {
            networks: deployment
                .networks
                .iter()
                .map(|net| {
                    let gateway = gateway(&net.ip, &net.mask)?;
                    Ok(Network {
                        ipv6: gateway.is_ipv6(),
                        addr: net.ip.clone(),
                        gateway: gateway.to_string(),
                        mask: net.mask.clone(),
                    })
                })
                .collect::<Result<_, Error>>()?,
            hosts: deployment.hosts.clone(),
        })
        .await
        .map_err(|e| Error::Other(format!("Network setup error: {:?}", e)))?;

    let endpoint = match response.endpoint {
        Some(endpoint) => endpoint,
        None => return Err(VpnError::EndpointInvalid("missing socket path".into()).into()),
    };

    let vpn = Vpn::try_new(
        activity_id,
        VpnEndpoint::connect(endpoint).await?,
        deployment,
    )?;

    Ok(Some(vpn.start()))
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
            ExeScriptCommand::Deploy { .. } => self.handle_process_command(cmd, address),
            _ => match &self.deployment.runtime_mode {
                RuntimeMode::ProcessPerCommand => self.handle_process_command(cmd, address),
                RuntimeMode::Service => self.handle_service_command(cmd, address),
            },
        }
    }
}

impl Handler<UpdateDeployment> for RuntimeProcess {
    type Result = <UpdateDeployment as Message>::Result;

    fn handle(&mut self, msg: UpdateDeployment, _: &mut Self::Context) -> Self::Result {
        if let Some(task_package) = msg.task_package {
            self.deployment.task_package = Some(task_package);
        }
        if let Some(runtime_mode) = msg.runtime_mode {
            self.deployment.runtime_mode = runtime_mode;
        }
        if let Some(networks) = msg.networks {
            self.deployment.networks.extend(networks.into_iter());
        }
        if let Some(hosts) = msg.hosts {
            self.deployment.hosts.extend(hosts.into_iter());
        }
        if let Some(nodes) = msg.nodes {
            self.deployment.nodes.extend(nodes.into_iter());
        }
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

impl Handler<SetVpnService> for RuntimeProcess {
    type Result = <SetVpnService as Message>::Result;

    fn handle(&mut self, msg: SetVpnService, _: &mut Self::Context) -> Self::Result {
        if let Some(vpn) = self.vpn.replace(msg.0) {
            vpn.do_send(Shutdown(ShutdownReason::Interrupted(0)));
        }
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

    fn handle(&mut self, msg: Shutdown, _: &mut Self::Context) -> Self::Result {
        let timeout = process_kill_timeout_seconds();
        let service = self.service.take();
        let vpn = self.vpn.take();
        let mut children = std::mem::replace(&mut self.children, HashSet::new());

        async move {
            if let Some(vpn) = vpn {
                let _ = vpn.send(msg).await;
            }
            if let Some(svc) = service {
                let _ = svc.service.shutdown().await;
            }
            let _ = future::join_all(children.drain().map(move |t| t.kill(timeout))).await;
            Ok(())
        }
        .boxed_local()
    }
}

#[derive(Clone, Hash, Eq, PartialEq, From)]
enum ChildProcess {
    #[from]
    Single { pid: u32 },
    #[from]
    Tree(ProcessTree),
    #[from]
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
struct SetVpnService(Addr<Vpn>);

#[derive(Message)]
#[rtype("()")]
struct AddChildProcess(ChildProcess);

#[derive(Message)]
#[rtype("()")]
struct RemoveChildProcess(ChildProcess);
