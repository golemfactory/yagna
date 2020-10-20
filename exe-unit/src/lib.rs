use actix::prelude::*;
use chrono::Utc;
use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, TryFutureExt};
use std::path::PathBuf;
use std::time::Duration;

use ya_agreement_utils::agreement::OfferTemplate;
use ya_client_model::activity::activity_state::StatePair;
use ya_client_model::activity::{ActivityUsage, ExeScriptCommand, RuntimeEvent, State};
use ya_core_model::activity;
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

pub mod agreement;
pub mod error;
mod handlers;
pub mod message;
pub mod metrics;
mod notify;
mod output;
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
    events: Channel<RuntimeEvent>,
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
            events: Channel::default(),
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

    pub fn offer_template(binary: PathBuf) -> Result<OfferTemplate> {
        use crate::runtime::process::RuntimeProcess;

        let runtime_template = RuntimeProcess::offer_template(binary)?;
        let supervisor_template = OfferTemplate::new(serde_json::json!({
            "golem.com.usage.vector": MetricsService::usage_vector(),
            "golem.activity.caps.transfer.protocol": TransferService::schemes(),
        }));

        Ok(supervisor_template.patch(runtime_template))
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

impl<R: Runtime> ExeUnit<R> {
    async fn exec(
        exec: activity::Exec,
        addr: Addr<Self>,
        runtime: Addr<R>,
        transfers: Addr<TransferService>,
        mut events: mpsc::Sender<RuntimeEvent>,
        mut control: oneshot::Receiver<()>,
    ) {
        for (idx, cmd) in exec.exe_script.into_iter().enumerate() {
            if let Ok(Some(_)) = control.try_recv() {
                log::warn!("Batch {} execution aborted", exec.batch_id);
                break;
            }

            let batch_id = exec.batch_id.clone();
            let evt = RuntimeEvent::started(batch_id.clone(), idx, cmd.clone());
            if let Err(e) = events.send(evt).await {
                log::error!("Unable to report event: {:?}", e);
            }

            let runtime_cmd = ExecuteCommand {
                batch_id: exec.batch_id.clone(),
                idx,
                command: cmd.clone(),
                tx: events.clone(),
            };
            let result = Self::exec_cmd(
                runtime_cmd,
                addr.clone(),
                runtime.clone(),
                transfers.clone(),
            )
            .await;

            let (return_code, message) = match result {
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
                let message = message.unwrap_or("reason unspecified".into());
                log::warn!("Batch {} execution interrupted: {}", batch_id, message);
                break;
            }
        }
    }

    async fn exec_cmd(
        runtime_cmd: ExecuteCommand,
        addr: Addr<Self>,
        runtime: Addr<R>,
        transfer_service: Addr<TransferService>,
    ) -> Result<()> {
        if let ExeScriptCommand::Terminate {} = &runtime_cmd.command {
            log::warn!("Terminating running ExeScripts");
            let exclude_batches = vec![runtime_cmd.batch_id];
            addr.send(Stop { exclude_batches }).await??;
            addr.send(SetState::from(State::Initialized)).await?;
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

        log::info!("Executing command: {:?}", runtime_cmd.command);

        addr.send(SetState::from(state_pre.clone())).await?;

        match &runtime_cmd.command {
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

        let exit_code = runtime.send(runtime_cmd.clone()).await??;
        if exit_code != 0 {
            return Err(Error::CommandExitCodeError(exit_code));
        }

        if let ExeScriptCommand::Deploy { .. } = &runtime_cmd.command {
            let mut runtime_mode = RuntimeMode::ProcessPerCommand;
            let stdout = addr
                .send(GetStdOut {
                    batch_id: runtime_cmd.batch_id.clone(),
                    idx: runtime_cmd.idx,
                })
                .await?;

            if let Some(output) = stdout {
                let deployment = deploy::DeployResult::from_bytes(output).map_err(|e| {
                    log::error!("Deploy failed: {}", e);
                    Error::CommandError(e.to_string())
                })?;
                transfer_service
                    .send(AddVolumes::new(deployment.vols))
                    .await??;
                runtime_mode = deployment.start_mode.into();
            }
            runtime.send(SetRuntimeMode(runtime_mode)).await??;
        }

        let state_cur = addr.send(GetState {}).await?.0;
        if state_cur != state_pre {
            return Err(StateError::UnexpectedState {
                current: state_cur,
                expected: state_pre,
            }
            .into());
        }

        addr.send(SetState::from(state_pre.1.unwrap())).await?;
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
            actix_rpc::bind::<activity::Exec>(&srv_id, addr.clone().recipient());
            actix_rpc::bind::<activity::GetState>(&srv_id, addr.clone().recipient());
            actix_rpc::bind::<activity::GetUsage>(&srv_id, addr.clone().recipient());
            actix_rpc::bind::<activity::GetRunningCommand>(&srv_id, addr.clone().recipient());
            actix_rpc::bind::<activity::GetExecBatchResults>(&srv_id, addr.clone().recipient());

            actix_rpc::binds::<activity::StreamExecBatchResults>(&srv_id, addr.clone().recipient());
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
                    log::warn!("Usage limit exceeded: {}", info);
                    exe_unit.do_send(Shutdown(ShutdownReason::UsageLimitExceeded(info)));
                }
                error => log::warn!("Unable to retrieve metrics: {:?}", error),
            },
        },
        Err(e) => log::warn!("Unable to report activity usage: {:?}", e),
    }
}
