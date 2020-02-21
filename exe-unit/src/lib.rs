pub mod error;
mod handlers;
pub mod message;
pub mod metrics;
pub mod runtime;
pub mod service;
pub mod state;

use crate::error::Error;
use crate::message::*;
use crate::runtime::*;
use crate::service::metrics::MetricsService;
use crate::service::transfer_service::{TransferService, TransferResource, DeployImage};
use crate::service::{ServiceAddr, ServiceControl};
use crate::state::{ExeUnitState, StateError};
use actix::prelude::*;
use futures::TryFutureExt;
use std::path::PathBuf;
use std::time::Duration;
use ya_core_model::activity::*;
use ya_model::activity::activity_state::StatePair;
use ya_model::activity::{
    ActivityUsage, CommandResult, ExeScriptCommand, ExeScriptCommandResult, State,
};
use ya_service_bus::{actix_rpc, RpcEndpoint, RpcMessage};

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
    pub fn new(ctx: ExeUnitContext, runtime: R) -> Self {
        let state = ExeUnitState::default();
        let transfers = TransferService::new(&ctx.work_dir, &ctx.cache_dir).start();
        let runtime = runtime.with_context(ctx.clone()).start();
        let metrics = MetricsService::default().start();

        ExeUnit {
            ctx,
            state,
            runtime: runtime.clone(),
            metrics: metrics.clone(),
            transfers: transfers.clone(),
            services: vec![
                Box::new(ServiceAddr::new(metrics)),
                Box::new(ServiceAddr::new(runtime)),
                Box::new(ServiceAddr::new(transfers)),
            ],
        }
    }

    fn report_usage(&mut self, context: &mut Context<Self>) {
        if let Some(activity_id) = &self.ctx.service_id {
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

    async fn shutdown(addr: &Addr<Self>, reason: ShutdownReason) {
        log::warn!("Initiating shutdown: {}", reason);

        if let Err(error) = addr.send(Shutdown(reason)).await {
            log::error!(
                "Unable to perform a graceful shutdown: {:?}. Terminating",
                error
            );
            System::current().stop();
        }
    }
}

#[derive(Clone, Debug)]
struct ExecCtx {
    batch_id: String,
    idx: usize,
    cmd: ExeScriptCommand,
}

impl<R: Runtime> ExeUnit<R> {
    async fn exec(addr: Addr<Self>, exe_unit: Addr<R>, transfer_service: Addr<TransferService>, exec: Exec) {
        for (idx, cmd) in exec.exe_script.into_iter().enumerate() {
            let ctx = ExecCtx {
                batch_id: exec.batch_id.clone(),
                idx,
                cmd,
            };

            if let Err(error) = Self::exec_cmd(addr.clone(), exe_unit.clone(), transfer_service.clone(), ctx.clone()).await {
                let cmd_result = ExeScriptCommandResult {
                    index: ctx.idx as u32,
                    result: Some(ya_model::activity::CommandResult::Error),
                    message: Some(error.to_string()),
                };
                let set_state = SetState {
                    state: None,
                    running_command: Some(None),
                    batch_result: Some((ctx.batch_id, cmd_result)),
                };

                if let Err(error) = addr.send(set_state).await {
                    log::error!(
                        "Unable to update the state during exec failure: {:?}",
                        error
                    );
                }

                let message = format!("Command interrupted: {}", error.to_string());
                Self::shutdown(&addr, ShutdownReason::Error(message)).await;
                break;
            }

            if let ExeScriptCommand::Terminate {} = &ctx.cmd {
                Self::shutdown(&addr, ShutdownReason::Finished).await;
                return;
            }
        }
    }

    async fn exec_cmd(addr: Addr<Self>, exe_unit: Addr<R>, transfer_service: Addr<TransferService>, ctx: ExecCtx) -> Result<()> {
        let state = addr.send(GetState {}).await?.0;
        let before_state = match (&state.0, &state.1) {
            (_, Some(_)) => {
                return Err(StateError::Busy(state).into());
            }
            (State::Terminated, _) => {
                return Err(StateError::InvalidState(state).into());
            }
            (State::New, _) => match &ctx.cmd {
                ExeScriptCommand::Deploy { .. } => StatePair(State::New, Some(State::Deployed)),
                _ => {
                    return Err(StateError::InvalidState(state).into());
                }
            },
            (State::Deployed, _) => match &ctx.cmd {
                ExeScriptCommand::Start { .. } => StatePair(State::Deployed, Some(State::Ready)),
                _ => {
                    return Err(StateError::InvalidState(state).into());
                }
            },
            (s, _) => match &ctx.cmd {
                ExeScriptCommand::Deploy { .. } | ExeScriptCommand::Start { .. } => {
                    return Err(StateError::InvalidState(state).into());
                }
                _ => StatePair(s.clone(), Some(*s)),
            },
        };

        addr.send(SetState {
            state: Some(before_state.clone()),
            running_command: Some(Some(ctx.cmd.clone().into())),
            batch_result: None,
        })
        .await?;

        Self::pre_exec(addr.clone(), exe_unit.clone(), transfer_service, ctx.clone()).await?;

        let exe_result = exe_unit.send(ExecCmd(ctx.cmd.clone())).await??;
        if let CommandResult::Error = exe_result.result {
            return Err(Error::CommandError(format!(
                "{:?} command error: {}",
                ctx.cmd,
                exe_result.stderr.unwrap_or("<no stderr output>".to_owned())
            )));
        }

        let sanity_state = addr.send(GetState {}).await?.0;
        if sanity_state != before_state {
            return Err(StateError::UnexpectedState {
                current: sanity_state,
                expected: before_state,
            }
            .into());
        }

        addr.send(SetState {
            state: Some(StatePair(before_state.1.unwrap(), None)),
            running_command: Some(None),
            batch_result: Some((ctx.batch_id.clone(), exe_result.into_exe_result(ctx.idx))),
        })
        .await?;

        Ok(())
    }

    async fn pre_exec(addr: Addr<Self>, exe_unit: Addr<R>, transfer_service: Addr<TransferService>, ctx: ExecCtx) -> Result<()> {
        if let ExeScriptCommand::Transfer {from, to} = &ctx.cmd {
            let msg = TransferResource{from: from.clone(), to: to.clone()};
            return Ok(transfer_service.send(msg).await??);
        }
        else if let ExeScriptCommand::Deploy {} = &ctx.cmd {
            return Ok(());
        }
        else {
            return Ok(());
        }
    }
}

impl<R: Runtime> Actor for ExeUnit<R> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();

        if let Some(s) = &self.ctx.service_id {
            actix_rpc::bind::<Exec>(&s, addr.clone().recipient());
            actix_rpc::bind::<GetActivityState>(&s, addr.clone().recipient());
            actix_rpc::bind::<GetActivityUsage>(&s, addr.clone().recipient());
            actix_rpc::bind::<GetRunningCommand>(&s, addr.clone().recipient());
            actix_rpc::bind::<GetExecBatchResults>(&s, addr.clone().recipient());
        }

        IntervalFunc::new(*DEFAULT_REPORT_INTERVAL, Self::report_usage)
            .finish()
            .spawn(ctx);

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
    pub service_id: Option<String>,
    pub report_url: Option<String>,
    pub agreement: PathBuf,
    pub work_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl ExeUnitContext {
    pub fn match_service(&self, service_id: &str) -> Result<()> {
        match &self.service_id {
            Some(sid) => match sid == service_id {
                true => Ok(()),
                false => Err(Error::RemoteServiceError(format!(
                    "Invalid destination service address: {}",
                    service_id
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
                let msg = SetActivityUsage {
                    activity_id,
                    usage: ActivityUsage::from(data),
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
