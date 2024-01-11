#[macro_use]
extern crate derive_more;

use std::path::PathBuf;
use std::time::Duration;

use actix::prelude::*;
use chrono::Utc;
use futures::channel::{mpsc, oneshot};
use futures::{FutureExt, SinkExt};

use ya_agreement_utils::agreement::OfferTemplate;
use ya_client_model::activity::{
    activity_state::StatePair, ActivityUsage, CommandOutput, ExeScriptCommand, State,
};
use ya_core_model::activity;
use ya_core_model::activity::local::Credentials;
use ya_runtime_api::deploy;
use ya_service_bus::{actix_rpc, RpcEndpoint, RpcMessage};
use ya_transfer::transfer::{
    AddVolumes, DeployImage, ForwardProgressToSink, TransferResource, TransferService,
    TransferServiceContext,
};

use crate::acl::Acl;
use crate::agreement::Agreement;
use crate::error::Error;
use crate::message::*;
use crate::runtime::*;
use crate::service::metrics::MetricsService;
use crate::service::{ServiceAddr, ServiceControl};
use crate::state::{ExeUnitState, StateError, Supervision};

mod acl;
pub mod agreement;
#[cfg(feature = "sgx")]
pub mod crypto;
pub mod error;
mod handlers;
pub mod logger;
pub mod manifest;
pub mod message;
pub mod metrics;
mod network;
mod notify;
mod output;
pub mod process;
pub mod runtime;
pub mod service;
pub mod state;

mod dns;
pub type Result<T> = std::result::Result<T, Error>;

lazy_static::lazy_static! {
    static ref DEFAULT_REPORT_INTERVAL: Duration = Duration::from_secs(1u64);
}

pub struct ExeUnit<R: Runtime> {
    ctx: ExeUnitContext,
    state: ExeUnitState,
    events: Channel<RuntimeEvent>,
    runtime: Addr<R>,
    metrics: Addr<MetricsService>,
    transfers: Addr<TransferService>,
    services: Vec<Box<dyn ServiceControl>>,
    shutdown_tx: Option<oneshot::Sender<Result<()>>>,
}

impl<R: Runtime> ExeUnit<R> {
    pub fn new(
        shutdown_tx: oneshot::Sender<Result<()>>,
        ctx: ExeUnitContext,
        metrics: Addr<MetricsService>,
        transfers: Addr<TransferService>,
        runtime: Addr<R>,
    ) -> Self {
        ExeUnit {
            ctx,
            state: ExeUnitState::default(),
            events: Channel::default(),
            runtime: runtime.clone(),
            metrics: metrics.clone(),
            transfers: transfers.clone(),
            services: vec![
                Box::new(ServiceAddr::new(metrics)),
                Box::new(ServiceAddr::new(transfers)),
                Box::new(ServiceAddr::new(runtime)),
            ],
            shutdown_tx: Some(shutdown_tx),
        }
    }

    pub fn offer_template(binary: PathBuf, args: Vec<String>) -> Result<OfferTemplate> {
        use crate::runtime::process::RuntimeProcess;

        let runtime_template = RuntimeProcess::offer_template(binary, args)?;
        let supervisor_template = OfferTemplate::new(serde_json::json!({
            "golem.com.usage.vector": MetricsService::usage_vector(),
            "golem.activity.caps.transfer.protocol": TransferService::schemes(),
            "golem.activity.caps.transfer.report-progress": true,
        }));

        Ok(supervisor_template.patch(runtime_template))
    }

    pub fn test(binary: PathBuf, args: Vec<String>) -> Result<std::process::Output> {
        use crate::runtime::process::RuntimeProcess;
        RuntimeProcess::test(binary, args)
    }

    fn report_usage(&mut self, context: &mut Context<Self>) {
        if self.ctx.activity_id.is_none() || self.ctx.report_url.is_none() {
            return;
        }
        let fut = report_usage(
            self.ctx.report_url.clone().unwrap(),
            self.ctx.activity_id.clone().unwrap(),
            context.address(),
            self.metrics.clone(),
        );
        context.spawn(fut.into_actor(self));
    }

    async fn stop_runtime(runtime: Addr<R>, reason: ShutdownReason) {
        if let Err(e) = runtime
            .send(Shutdown(reason))
            .timeout(Duration::from_secs(5u64))
            .await
        {
            log::warn!("Unable to stop the runtime: {:?}", e);
        }
    }
}

#[derive(Clone)]
struct RuntimeRef<R: Runtime>(Addr<ExeUnit<R>>);

impl<R: Runtime> RuntimeRef<R> {
    fn from_ctx(ctx: &Context<ExeUnit<R>>) -> Self {
        RuntimeRef(ctx.address())
    }
}

impl<R: Runtime> std::ops::Deref for RuntimeRef<R> {
    type Target = Addr<ExeUnit<R>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<R: Runtime> RuntimeRef<R> {
    async fn exec(
        self,
        exec: activity::Exec,
        runtime: Addr<R>,
        transfers: Addr<TransferService>,
        mut events: mpsc::Sender<RuntimeEvent>,
        mut control: oneshot::Receiver<()>,
    ) {
        let batch_id = exec.batch_id.clone();
        for (idx, command) in exec.exe_script.into_iter().enumerate() {
            if let Ok(Some(_)) = control.try_recv() {
                log::warn!("Batch {} execution aborted", batch_id);
                break;
            }

            let runtime_cmd = ExecuteCommand {
                batch_id: batch_id.clone(),
                command: command.clone(),
                tx: events.clone(),
                idx,
            };

            let evt = RuntimeEvent::started(batch_id.clone(), idx, command.clone());
            if let Err(e) = events.send(evt).await {
                log::error!("Unable to report event: {:?}", e);
            }

            let (return_code, message) = match {
                if runtime_cmd.stateless() {
                    self.exec_stateless(&runtime_cmd).await
                } else {
                    self.exec_stateful(runtime_cmd, &runtime, &transfers).await
                }
            } {
                Ok(_) => (0, None),
                Err(ref err) => match err {
                    Error::CommandExitCodeError(c) => (*c, Some(err.to_string())),
                    _ => (-1, Some(err.to_string())),
                },
            };

            let evt = RuntimeEvent::finished(batch_id.clone(), idx, return_code, message.clone());
            if let Err(e) = events.send(evt).await {
                log::error!("Unable to report event: {:?}", e);
            }

            if return_code != 0 {
                let message = message.unwrap_or_else(|| "reason unspecified".into());
                log::warn!("Batch {} execution interrupted: {}", batch_id, message);
                break;
            }
        }
    }

    async fn exec_stateless(&self, runtime_cmd: &ExecuteCommand) -> Result<()> {
        match runtime_cmd.command {
            ExeScriptCommand::Sign {} => {
                let batch_id = runtime_cmd.batch_id.clone();
                let signature = self.send(SignExeScript { batch_id }).await??;
                let stdout = serde_json::to_string(&signature)?;

                runtime_cmd
                    .tx
                    .clone()
                    .send(RuntimeEvent::stdout(
                        runtime_cmd.batch_id.clone(),
                        runtime_cmd.idx,
                        CommandOutput::Bin(stdout.into_bytes()),
                    ))
                    .await
                    .map_err(|e| Error::runtime(format!("Unable to send stdout event: {:?}", e)))?;
            }
            ExeScriptCommand::Terminate {} => {
                log::debug!("Terminating running ExeScripts");
                let exclude_batches = vec![runtime_cmd.batch_id.clone()];
                self.send(Stop { exclude_batches }).await??;
                self.send(SetState::from(State::Initialized)).await?;
            }
            _ => (),
        }
        Ok(())
    }

    async fn exec_stateful(
        &self,
        runtime_cmd: ExecuteCommand,
        runtime: &Addr<R>,
        transfer_service: &Addr<TransferService>,
    ) -> Result<()> {
        let state = self.send(GetState {}).await?.0;
        let state_pre = match (&state.0, &state.1) {
            (_, Some(_)) => {
                return Err(StateError::Busy(state).into());
            }
            (State::New, _) | (State::Terminated, _) => {
                return Err(StateError::InvalidState(state).into());
            }
            (State::Initialized, _) => match &runtime_cmd.command {
                ExeScriptCommand::Deploy { .. } => {
                    StatePair(State::Initialized, Some(State::Deployed))
                }
                _ => return Err(StateError::InvalidState(state).into()),
            },
            (State::Deployed, _) => match &runtime_cmd.command {
                ExeScriptCommand::Start { .. } => StatePair(State::Deployed, Some(State::Ready)),
                _ => return Err(StateError::InvalidState(state).into()),
            },
            (s, _) => match &runtime_cmd.command {
                ExeScriptCommand::Deploy { .. } | ExeScriptCommand::Start { .. } => {
                    return Err(StateError::InvalidState(state).into());
                }
                _ => StatePair(*s, Some(*s)),
            },
        };
        self.send(SetState::from(state_pre)).await?;

        log::info!("Executing command: {:?}", runtime_cmd.command);

        let result = async {
            self.pre_runtime(&runtime_cmd, runtime, transfer_service)
                .await?;

            let exit_code = runtime.send(runtime_cmd.clone()).await??;
            if exit_code != 0 {
                return Err(Error::CommandExitCodeError(exit_code));
            }

            self.post_runtime(&runtime_cmd, runtime, transfer_service)
                .await?;

            Ok(())
        }
        .await;

        let state_cur = self.send(GetState {}).await?.0;
        if state_cur != state_pre {
            return Err(StateError::UnexpectedState {
                current: state_cur,
                expected: state_pre,
            }
            .into());
        }

        self.send(SetState::from(state_pre.1.unwrap())).await?;
        result
    }

    async fn pre_runtime(
        &self,
        runtime_cmd: &ExecuteCommand,
        runtime: &Addr<R>,
        transfer_service: &Addr<TransferService>,
    ) -> Result<()> {
        match &runtime_cmd.command {
            ExeScriptCommand::Transfer {
                from,
                to,
                args,
                progress,
            } => {
                let mut msg = TransferResource {
                    from: from.clone(),
                    to: to.clone(),
                    args: args.clone(),
                    progress_config: None,
                };

                if let Some(args) = progress {
                    msg.forward_progress(args, runtime_cmd.progress_sink())
                }
                transfer_service.send(msg).await??;
            }
            ExeScriptCommand::Deploy {
                net,
                hosts,
                progress,
                ..
            } => {
                // TODO: We should pass `task_package` here not in `TransferService` initialization.
                let mut msg = DeployImage::default();
                if let Some(args) = progress {
                    msg.forward_progress(args, runtime_cmd.progress_sink())
                }

                let task_package = transfer_service.send(msg).await??;
                runtime
                    .send(UpdateDeployment {
                        task_package,
                        networks: Some(net.clone()),
                        hosts: Some(hosts.clone()),
                        ..Default::default()
                    })
                    .await??;
            }
            _ => (),
        }
        Ok(())
    }

    async fn post_runtime(
        &self,
        runtime_cmd: &ExecuteCommand,
        runtime: &Addr<R>,
        transfer_service: &Addr<TransferService>,
    ) -> Result<()> {
        if let ExeScriptCommand::Deploy { .. } = &runtime_cmd.command {
            let mut runtime_mode = RuntimeMode::ProcessPerCommand;
            let stdout = self
                .send(GetStdOut {
                    batch_id: runtime_cmd.batch_id.clone(),
                    idx: runtime_cmd.idx,
                })
                .await?;

            if let Some(output) = stdout {
                let deployment = deploy::DeployResult::from_bytes(output).map_err(|e| {
                    log::error!("Deployment failed: {}", e);
                    Error::CommandError(e.to_string())
                })?;
                transfer_service
                    .send(AddVolumes::new(deployment.vols))
                    .await??;
                runtime_mode = deployment.start_mode.into();
            }
            runtime
                .send(UpdateDeployment {
                    runtime_mode: Some(runtime_mode),
                    ..Default::default()
                })
                .await??;
        }
        Ok(())
    }
}

impl<R: Runtime> Actor for ExeUnit<R> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let rx = self.events.rx.take().unwrap();
        Self::add_stream(rx, ctx);

        let addr = ctx.address();
        if let Some(activity_id) = &self.ctx.activity_id {
            let srv_id = activity::exeunit::bus_id(activity_id);
            actix_rpc::bind::<activity::GetState>(&srv_id, addr.clone().recipient());
            actix_rpc::bind::<activity::GetUsage>(&srv_id, addr.clone().recipient());

            #[cfg(feature = "sgx")]
            {
                actix_rpc::bind::<activity::sgx::CallEncryptedService>(
                    &srv_id,
                    addr.clone().recipient(),
                );
            }
            #[cfg(not(feature = "sgx"))]
            {
                actix_rpc::bind::<activity::Exec>(&srv_id, addr.clone().recipient());
                actix_rpc::bind::<activity::GetExecBatchResults>(&srv_id, addr.clone().recipient());
                actix_rpc::bind::<activity::GetRunningCommand>(&srv_id, addr.clone().recipient());
                actix_rpc::binds::<activity::StreamExecBatchResults>(
                    &srv_id,
                    addr.clone().recipient(),
                );
            }
        }

        IntervalFunc::new(*DEFAULT_REPORT_INTERVAL, Self::report_usage)
            .finish()
            .spawn(ctx);

        log::info!("Initializing manifests");
        self.ctx
            .supervise
            .manifest
            .build_validators()
            .into_actor(self)
            .map(|result, this, ctx| match result {
                Ok(validators) => {
                    this.ctx.supervise.manifest.add_validators(validators);
                    log::info!("Manifest initialization complete");
                }
                Err(e) => {
                    let err = Error::Other(format!("manifest initialization error: {}", e));
                    log::error!("Supervisor is shutting down due to {}", err);
                    ctx.address().do_send(Shutdown(ShutdownReason::Error(err)));
                }
            })
            .wait(ctx);

        let addr_ = addr.clone();
        async move {
            addr.send(Initialize).await?.map_err(Error::from)?;
            addr.send(SetState::from(State::Initialized)).await?;
            Ok::<_, Error>(())
        }
        .then(|result| async move {
            match result {
                Ok(_) => log::info!("Supervisor initialized"),
                Err(e) => {
                    let err = Error::Other(format!("initialization error: {}", e));
                    log::error!("Supervisor is shutting down due to {}", err);
                    let _ = addr_.send(Shutdown(ShutdownReason::Error(err))).await;
                }
            }
        })
        .into_actor(self)
        .spawn(ctx);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        if self.state.inner.0 == State::Terminated {
            return Running::Stop;
        }
        Running::Continue
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(Ok(()));
        }
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct ExeUnitContext {
    pub supervise: Supervision,
    pub activity_id: Option<String>,
    pub report_url: Option<String>,
    pub agreement: Agreement,
    pub work_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub runtime_args: Vec<String>,
    pub acl: Acl,
    pub credentials: Option<Credentials>,
    #[cfg(feature = "sgx")]
    #[derivative(Debug = "ignore")]
    pub crypto: crypto::Crypto,
}

impl ExeUnitContext {
    pub fn verify_activity_id(&self, activity_id: &str) -> Result<()> {
        match &self.activity_id {
            Some(act_id) => match act_id == activity_id {
                true => Ok(()),
                false => Err(Error::RemoteServiceError(format!(
                    "Forbidden! Invalid activity id: {}",
                    activity_id
                ))),
            },
            None => Ok(()),
        }
    }
}

impl From<&ExeUnitContext> for TransferServiceContext {
    fn from(val: &ExeUnitContext) -> Self {
        TransferServiceContext {
            task_package: val.agreement.task_package.clone(),
            deploy_retry: None,
            cache_dir: val.cache_dir.clone(),
            work_dir: val.work_dir.clone(),
            transfer_retry: None,
        }
    }
}

struct Channel<T> {
    tx: mpsc::Sender<T>,
    rx: Option<mpsc::Receiver<T>>,
}

impl<T> Default for Channel<T> {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel(8);
        Channel { tx, rx: Some(rx) }
    }
}

pub(crate) async fn report<S, M>(url: S, msg: M) -> bool
where
    M: RpcMessage + Unpin + 'static,
    S: AsRef<str>,
{
    let url = url.as_ref();
    match ya_service_bus::typed::service(url).send(msg).await {
        Err(ya_service_bus::Error::Timeout(msg)) => {
            log::warn!("Timed out reporting to {}: {}", url, msg);
            true
        }
        Err(e) => {
            log::error!("Error reporting to {}: {:?}", url, e);
            false
        }
        Ok(Err(e)) => {
            log::error!("Error response while reporting to {}: {:?}", url, e);
            false
        }
        Ok(Ok(_)) => true,
    }
}

async fn report_usage<R: Runtime>(
    report_url: String,
    activity_id: String,
    exe_unit: Addr<ExeUnit<R>>,
    metrics: Addr<MetricsService>,
) {
    match metrics.send(GetMetrics).await {
        Ok(resp) => match resp {
            Ok(data) => {
                let msg = activity::local::SetUsage {
                    activity_id,
                    usage: ActivityUsage {
                        current_usage: Some(data),
                        timestamp: Utc::now().timestamp(),
                    },
                    timeout: None,
                };
                if !report(&report_url, msg).await {
                    exe_unit.do_send(Shutdown(ShutdownReason::Error(Error::RuntimeError(
                        format!("Reporting endpoint '{}' is not available", report_url),
                    ))));
                }
            }
            Err(err) => match err {
                Error::UsageLimitExceeded(info) => {
                    log::warn!("Usage limit exceeded: {}", info);
                    exe_unit.do_send(Shutdown(ShutdownReason::UsageLimitExceeded(info)));
                }
                error => log::warn!("Unable to retrieve metrics: {:?}", error),
            },
        },
        Err(e) => log::warn!("Unable to report activity usage: {:?}", e),
    }
}

impl Handler<Shutdown> for TransferService {
    type Result = ResponseFuture<Result<()>>;

    fn handle(&mut self, _msg: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        let addr = ctx.address();
        async move { Ok(addr.send(ya_transfer::transfer::Shutdown {}).await??) }.boxed_local()
    }
}
