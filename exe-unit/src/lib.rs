use actix::prelude::*;
use futures::channel::oneshot;
use futures::{FutureExt, TryFutureExt};
use std::path::PathBuf;
use std::time::Duration;

use ya_client_model::activity::{
    activity_state::StatePair, ActivityUsage, CommandResult, ExeScriptCommand,
    ExeScriptCommandResult, State,
};
use ya_core_model::activity;
use ya_core_model::activity::local::Credentials;
use ya_runtime_api::deploy;
use ya_service_bus::{actix_rpc, RpcEndpoint, RpcMessage};

use crate::agreement::Agreement;
use crate::error::Error;
use crate::message::*;
use crate::runtime::*;
use crate::service::metrics::MetricsService;
use crate::service::transfer::{AddVolumes, DeployImage, TransferResource, TransferService};
use crate::service::{ServiceAddr, ServiceControl};
use crate::state::{ExeUnitState, StateError};
use chrono::Utc;

pub mod agreement;
#[cfg(feature = "sgx")]
pub mod crypto;
pub mod error;
mod handlers;
pub mod message;
pub mod metrics;
mod notify;
pub mod process;
pub mod runtime;
pub mod service;
pub mod state;
pub mod util;

pub type Result<T> = std::result::Result<T, Error>;

lazy_static::lazy_static! {
    static ref DEFAULT_REPORT_INTERVAL: Duration = Duration::from_secs(1u64);
}

pub struct ExeUnit<R: Runtime> {
    ctx: ExeUnitContext,
    state: ExeUnitState,
    runtime: Addr<R>,
    metrics: Addr<MetricsService>,
    transfers: Addr<TransferService>,
    services: Vec<Box<dyn ServiceControl>>,
}

impl<R: Runtime> ExeUnit<R> {
    pub fn new(
        ctx: ExeUnitContext,
        metrics: Addr<MetricsService>,
        transfers: Addr<TransferService>,
        runtime: Addr<R>,
    ) -> Self {
        ExeUnit {
            ctx,
            state: ExeUnitState::default(),
            runtime: runtime.clone(),
            metrics: metrics.clone(),
            transfers: transfers.clone(),
            services: vec![
                Box::new(ServiceAddr::new(metrics)),
                Box::new(ServiceAddr::new(transfers)),
                Box::new(ServiceAddr::new(runtime)),
            ],
        }
    }

    fn report_usage(&mut self, context: &mut Context<Self>) {
        if let Some(activity_id) = &self.ctx.activity_id {
            let fut = report_usage(
                self.ctx.report_url.clone().unwrap(),
                activity_id.clone(),
                context.address(),
                self.metrics.clone(),
            );
            context.spawn(fut.into_actor(self));
        };
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

#[derive(Clone, Debug)]
struct ExeCtx {
    batch_id: String,
    batch_size: usize,
    idx: usize,
    cmd: ExeScriptCommand,
}

impl ExeCtx {
    pub fn convert_runtime_result(&self, result: RuntimeCommandResult) -> ExeScriptCommandResult {
        let stdout = result
            .stdout
            .filter(|s| !s.is_empty())
            .map(|s| format!("stdout: {}", s));
        let stderr = result
            .stderr
            .filter(|s| !s.is_empty())
            .map(|s| format!("stderr: {}", s));
        let message = match (stdout, stderr) {
            (None, None) => None,
            (Some(stdout), None) => Some(stdout),
            (None, Some(stderr)) => Some(stderr),
            (Some(stdout), Some(stderr)) => Some(format!("{}\n{}", stdout, stderr)),
        };
        let finished = self.idx == self.batch_size - 1 || result.result == CommandResult::Error;
        ExeScriptCommandResult {
            index: self.idx as u32,
            result: result.result,
            is_batch_finished: finished,
            message,
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
        runtime: Addr<R>,
        transfers: Addr<TransferService>,
        exec: activity::Exec,
        mut control: oneshot::Receiver<()>,
    ) {
        let batch_size = exec.exe_script.len();
        let on_error = |batch_id, result| async {
            let set_state = SetState::default().cmd(None).result(batch_id, result);
            if let Err(error) = self.send(set_state).await {
                log::error!("Cannot update state during exec: {:?}", error);
            }
        };

        for (idx, cmd) in exec.exe_script.into_iter().enumerate() {
            let ctx = ExeCtx {
                batch_id: exec.batch_id.clone(),
                batch_size,
                idx,
                cmd,
            };

            if let Ok(Some(_)) = control.try_recv() {
                let cmd_result =
                    ctx.convert_runtime_result(RuntimeCommandResult::error("interrupted"));
                on_error(ctx.batch_id, cmd_result).await;
                break;
            }

            if let Err(error) = self
                .exec_cmd(runtime.clone(), transfers.clone(), ctx.clone())
                .await
            {
                log::warn!("Command interrupted: {}", error.to_string());
                let cmd_result = ctx.convert_runtime_result(RuntimeCommandResult::error(&error));
                on_error(ctx.batch_id, cmd_result).await;
                break;
            }
        }
    }

    async fn exec_cmd(
        &self,
        runtime: Addr<R>,
        transfer_service: Addr<TransferService>,
        ctx: ExeCtx,
    ) -> Result<()> {
        if let ExeScriptCommand::Terminate {} = &ctx.cmd {
            log::warn!("Terminating running ExeScripts");

            let exclude_batches = vec![ctx.batch_id];
            let set_state = SetState::default()
                .state(StatePair(State::Initialized, None))
                .cmd(None);

            self.send(Stop { exclude_batches }).await??;
            self.send(set_state).await?;
            return Ok(());
        }

        let state = self.send(GetState {}).await?.0;
        let state_pre = match (&state.0, &state.1) {
            (_, Some(_)) => {
                return Err(StateError::Busy(state).into());
            }
            (State::New, _) | (State::Terminated, _) => {
                return Err(StateError::InvalidState(state).into());
            }
            (State::Initialized, _) => match &ctx.cmd {
                ExeScriptCommand::Deploy { .. } => {
                    StatePair(State::Initialized, Some(State::Deployed))
                }
                _ => return Err(StateError::InvalidState(state).into()),
            },
            (State::Deployed, _) => match &ctx.cmd {
                ExeScriptCommand::Start { .. } => StatePair(State::Deployed, Some(State::Ready)),
                _ => return Err(StateError::InvalidState(state).into()),
            },
            (s, _) => match &ctx.cmd {
                ExeScriptCommand::Deploy { .. } | ExeScriptCommand::Start { .. } => {
                    return Err(StateError::InvalidState(state).into());
                }
                _ => StatePair(*s, Some(*s)),
            },
        };

        log::info!("Executing command: {:?}", ctx.cmd);

        self.send(
            SetState::default()
                .state(state_pre.clone())
                .cmd(Some(ctx.cmd.clone())),
        )
        .await?;

        match &ctx.cmd {
            ExeScriptCommand::Transfer { from, to, args } => {
                let msg = TransferResource {
                    from: from.clone(),
                    to: to.clone(),
                    args: args.clone(),
                };
                transfer_service.send(msg).await??;
            }
            ExeScriptCommand::Deploy {} => {
                let msg = DeployImage {};
                let path = transfer_service.send(msg).await??;
                runtime.send(SetTaskPackagePath(path)).await?;
            }
            _ => (),
        }

        let runtime_result = runtime.send(RuntimeCommand(ctx.cmd.clone())).await??;

        if let ExeScriptCommand::Deploy { .. } = &ctx.cmd {
            let mut runtime_mode = RuntimeMode::ProcessPerCommand;
            if let Some(output) = &runtime_result.stdout {
                let deployment = match deploy::DeployResult::from_bytes(output) {
                    Ok(v) => v,
                    Err(e) => {
                        log::error!("Deploy failed: {}", e);
                        return Err(Error::CommandError(runtime_result));
                    }
                };
                log::info!("Adding volumes: {:?}", deployment.vols);
                transfer_service
                    .send(AddVolumes::new(deployment.vols))
                    .await??;
                runtime_mode = deployment.start_mode.into();
            }
            runtime.send(SetRuntimeMode(runtime_mode)).await??;
        }

        if let CommandResult::Error = runtime_result.result {
            return Err(Error::CommandError(runtime_result));
        }

        let state_cur = self.send(GetState {}).await?.0;
        if state_cur != state_pre {
            return Err(StateError::UnexpectedState {
                current: state_cur,
                expected: state_pre,
            }
            .into());
        }

        let cmd_result = ctx.convert_runtime_result(runtime_result);
        let state_post = SetState::default()
            .state(state_pre.1.unwrap().into())
            .cmd(None)
            .result(ctx.batch_id, cmd_result);
        self.send(state_post).await?;

        Ok(())
    }
}

impl<R: Runtime> Actor for ExeUnit<R> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
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
            }
        }

        IntervalFunc::new(*DEFAULT_REPORT_INTERVAL, Self::report_usage)
            .finish()
            .spawn(ctx);

        let fut = async move {
            addr.send(Initialize).await?.map_err(Error::from)?;
            addr.send(SetState::from(State::Initialized)).await?;
            Ok(())
        }
        .map_err(|e: Error| panic!("Supervisor initialization error: {}", e))
        .map(|_| log::info!("Started"));

        ctx.spawn(fut.into_actor(self));
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        if self.state.inner.0 == State::Terminated {
            return Running::Stop;
        }
        Running::Continue
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
#[derive(Clone)]
pub struct ExeUnitContext {
    pub activity_id: Option<String>,
    pub report_url: Option<String>,
    pub credentials: Option<Credentials>,
    pub agreement: Agreement,
    pub work_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub runtime_args: RuntimeArgs,
    #[cfg(feature = "sgx")]
    #[derivative(Debug = "ignore")]
    pub crypto: crate::crypto::Crypto,
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

pub(crate) async fn report<M: RpcMessage + Unpin + 'static>(url: String, msg: M) {
    let result = ya_service_bus::typed::service(&url)
        .send(msg)
        .map_err(Error::from)
        .await;

    if let Err(e) = result {
        log::warn!("Error reporting to {}: {:?}", url, e);
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
                report(report_url, msg).await;
            }
            Err(err) => match err {
                Error::UsageLimitExceeded(info) => {
                    exe_unit.do_send(Shutdown(ShutdownReason::UsageLimitExceeded(info)));
                }
                error => log::warn!("Unable to retrieve metrics: {:?}", error),
            },
        },
        Err(e) => log::warn!("Unable to report activity usage: {:?}", e),
    }
}
