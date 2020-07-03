use actix::prelude::*;
use futures::channel::oneshot;
use futures::TryFutureExt;
use std::path::PathBuf;
use std::time::Duration;

use ya_client_model::activity::activity_state::StatePair;
use ya_client_model::activity::{
    ActivityUsage, CommandResult, ExeScriptCommand, ExeScriptCommandResult, State,
};
use ya_core_model::activity;
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
use ya_runtime_api::deploy;
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
struct ExecCtx {
    batch_id: String,
    batch_size: usize,
    idx: usize,
    cmd: ExeScriptCommand,
}

impl ExecCtx {
    pub fn cmd_result(&self, exec_result: ExecCmdResult) -> ExeScriptCommandResult {
        let stdout = exec_result
            .stdout
            .filter(|s| !s.is_empty())
            .map(|s| format!("stdout: {}", s));
        let stderr = exec_result
            .stderr
            .filter(|s| !s.is_empty())
            .map(|s| format!("stderr: {}", s));
        let message = match (stdout, stderr) {
            (None, None) => None,
            (Some(stdout), None) => Some(stdout),
            (None, Some(stderr)) => Some(stderr),
            (Some(stdout), Some(stderr)) => Some(format!("{}\n{}", stdout, stderr)),
        };
        let finished =
            self.idx == self.batch_size - 1 || exec_result.result == CommandResult::Error;
        ExeScriptCommandResult {
            index: self.idx as u32,
            result: exec_result.result,
            is_batch_finished: finished,
            message,
        }
    }
}

impl<R: Runtime> ExeUnit<R> {
    async fn exec(
        addr: Addr<Self>,
        runtime: Addr<R>,
        transfers: Addr<TransferService>,
        exec: activity::Exec,
        mut control: oneshot::Receiver<()>,
    ) {
        let batch_size = exec.exe_script.len();
        let on_error = |batch_id, result| async {
            let set_state = SetState::default().cmd(None).result(batch_id, result);
            if let Err(error) = addr.send(set_state).await {
                log::error!("Cannot update state during exec: {:?}", error);
            }
        };

        for (idx, cmd) in exec.exe_script.into_iter().enumerate() {
            let ctx = ExecCtx {
                batch_id: exec.batch_id.clone(),
                batch_size,
                idx,
                cmd,
            };

            if let Ok(Some(_)) = control.try_recv() {
                let cmd_result = ctx.cmd_result(ExecCmdResult::error("interrupted"));
                on_error(ctx.batch_id, cmd_result).await;
                break;
            }

            if let Err(error) = Self::exec_cmd(
                addr.clone(),
                runtime.clone(),
                transfers.clone(),
                ctx.clone(),
            )
            .await
            {
                log::warn!("Command interrupted: {}", error.to_string());
                let cmd_result = ctx.cmd_result(ExecCmdResult::error(&error));
                on_error(ctx.batch_id, cmd_result).await;
                break;
            }
        }
    }

    async fn exec_cmd(
        addr: Addr<Self>,
        runtime: Addr<R>,
        transfer_service: Addr<TransferService>,
        ctx: ExecCtx,
    ) -> Result<()> {
        if let ExeScriptCommand::Terminate {} = &ctx.cmd {
            log::warn!("Terminating running ExeScripts");

            let exclude_batches = vec![ctx.batch_id];
            let set_state = SetState::default()
                .state(StatePair(State::Initialized, None))
                .cmd(None);

            addr.send(Stop { exclude_batches }).await??;
            addr.send(set_state).await?;
            return Ok(());
        }

        let state = addr.send(GetState {}).await?.0;
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

        addr.send(
            SetState::default()
                .state(state_pre.clone())
                .cmd(Some(ctx.cmd.clone())),
        )
        .await?;

        match &ctx.cmd {
            ExeScriptCommand::Transfer { from, to } => {
                let msg = TransferResource {
                    from: from.clone(),
                    to: to.clone(),
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

        let exec_result = runtime.send(ExecCmd(ctx.cmd.clone())).await??;
        if let ExeScriptCommand::Deploy { .. } = &ctx.cmd {
            if let Some(output) = &exec_result.stdout {
                let deploy_desc = match deploy::DeployResult::from_bytes(output) {
                    Ok(v) => v,
                    Err(e) => {
                        log::error!("deploy failed: {}", e);
                        return Err(Error::CommandError(exec_result));
                    }
                };
                log::info!("adding vols: {:?}", deploy_desc.vols);
                transfer_service
                    .send(AddVolumes::new(deploy_desc.vols))
                    .await??;
            }
        }

        if let CommandResult::Error = exec_result.result {
            return Err(Error::CommandError(exec_result));
        }

        let state_cur = addr.send(GetState {}).await?.0;
        if state_cur != state_pre {
            return Err(StateError::UnexpectedState {
                current: state_cur,
                expected: state_pre,
            }
            .into());
        }

        let cmd_result = ctx.cmd_result(exec_result);
        let state_post = SetState::default()
            .state(state_pre.1.unwrap().into())
            .cmd(None)
            .result(ctx.batch_id, cmd_result);
        addr.send(state_post).await?;

        Ok(())
    }
}

impl<R: Runtime> Actor for ExeUnit<R> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();

        if let Some(activity_id) = &self.ctx.activity_id {
            let srv_id = activity::exeunit::bus_id(activity_id);
            actix_rpc::bind::<activity::Exec>(&srv_id, addr.clone().recipient());
            actix_rpc::bind::<activity::GetState>(&srv_id, addr.clone().recipient());
            actix_rpc::bind::<activity::GetUsage>(&srv_id, addr.clone().recipient());
            actix_rpc::bind::<activity::GetRunningCommand>(&srv_id, addr.clone().recipient());
            actix_rpc::bind::<activity::GetExecBatchResults>(&srv_id, addr.clone().recipient());
        }

        IntervalFunc::new(*DEFAULT_REPORT_INTERVAL, Self::report_usage)
            .finish()
            .spawn(ctx);

        addr.do_send(SetState::from(State::Initialized));
        log::info!("Started");
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        if self.state.inner.0 == State::Terminated {
            return Running::Stop;
        }
        Running::Continue
    }
}

#[derive(Clone, Debug)]
pub struct ExeUnitContext {
    pub activity_id: Option<String>,
    pub report_url: Option<String>,
    pub agreement: Agreement,
    pub work_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub runtime_args: RuntimeArgs,
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
