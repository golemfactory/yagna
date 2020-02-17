pub mod cli;
pub mod commands;
pub mod error;
mod handlers;
#[cfg(feature = "metrics")]
pub mod metrics;
pub mod runtime;
pub mod service;

use crate::commands::*;
use crate::error::Error;
use crate::runtime::*;
use crate::service::{ServiceAddr, ServiceControl};

use crate::service::metrics::MetricsService;
use actix::prelude::*;
use futures::TryFutureExt;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use ya_core_model::activity as activity_model;
use ya_core_model::activity::SetActivityUsage;
use ya_model::activity::{ActivityUsage, ExeScriptCommandResult, ExeScriptCommandState, State};
use ya_service_bus::{actix_rpc, RpcEndpoint, RpcMessage};

pub type Result<T> = std::result::Result<T, Error>;
pub type BatchResult = ExeScriptCommandResult;

lazy_static::lazy_static! {
    static ref DEFAULT_REPORT_INTERVAL: Duration = Duration::from_secs(1u64);
}

#[derive(Clone, Debug)]
pub struct ExeUnitContext {
    service_id: Option<String>,
    config_path: Option<PathBuf>,
    work_dir: PathBuf,
    cache_dir: PathBuf,
}

pub struct ExeUnitState {
    pub state: StateExt,
    batch_results: HashMap<String, Vec<BatchResult>>,
    pub running_command: Option<ExeScriptCommandState>,
}

impl ExeUnitState {
    pub fn get_results(&self, batch_id: &String) -> Vec<BatchResult> {
        match self.batch_results.get(batch_id) {
            Some(vec) => vec.clone(),
            None => Vec::new(),
        }
    }

    pub fn push_result(&mut self, batch_id: String, result: BatchResult) {
        match self.batch_results.get_mut(&batch_id) {
            Some(vec) => vec.push(result),
            None => {
                self.batch_results.insert(batch_id, vec![result]);
            }
        }
    }
}

impl Default for ExeUnitState {
    fn default() -> Self {
        ExeUnitState {
            state: StateExt::default(),
            batch_results: HashMap::new(),
            running_command: None,
        }
    }
}

pub struct ExeUnit<R: Runtime> {
    ctx: ExeUnitContext,
    state: ExeUnitState,
    runtime: Option<RuntimeThread<R>>,
    metrics: Addr<MetricsService>,
    report_url: String,
    services: Vec<Box<dyn ServiceControl>>,
}

macro_rules! actix_rpc_bind {
    ($sid:expr, $addr:expr, [$($ty:ty),*]) => {
        $(
            actix_rpc::bind::<$ty>($sid, $addr.clone().recipient());
        )*
    };
}

impl<R: Runtime> ExeUnit<R> {
    pub fn new(ctx: ExeUnitContext, report_url: String) -> Self {
        let metrics = MetricsService::default().start();
        ExeUnit {
            ctx,
            state: ExeUnitState::default(),
            runtime: None,
            metrics: metrics.clone(),
            report_url,
            services: vec![Box::new(ServiceAddr::new(metrics))],
        }
    }

    fn report_usage(&mut self, context: &mut Context<Self>) {
        let actor = context.address();
        let report_url = self.report_url.clone();
        let metrics = self.metrics.clone();
        let activity_id = match &self.ctx.service_id {
            Some(id) => id.clone(),
            None => return,
        };

        let fut = async move {
            let resp = metrics.send(MetricsRequest).await;
            if let Err(e) = resp {
                log::warn!("Unable to report activity usage: {:?}", e);
                return;
            };

            match resp.unwrap() {
                Ok(data) => {
                    let msg = SetActivityUsage {
                        activity_id,
                        usage: ActivityUsage {
                            current_usage: Some(data),
                        },
                        timeout: None,
                    };
                    report(&report_url, msg).await;
                }
                Err(e) => match e {
                    Error::UsageLimitExceeded(exceeded) => {
                        actor.do_send(Shutdown(ShutdownReason::UsageLimitExceeded(exceeded)));
                    }
                    _ => (),
                },
            };
        };

        context.spawn(fut.into_actor(self));
    }

    fn start_runtime(&mut self) -> Result<()> {
        let config_path = self.ctx.config_path.clone();
        let input_dir = self.ctx.work_dir.clone();
        let output_dir = self.ctx.cache_dir.clone();
        let runtime = RuntimeThread::spawn(move || {
            R::new(config_path.clone(), input_dir.clone(), output_dir.clone())
        })?;
        self.runtime = Some(runtime);
        Ok(())
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

    fn check_service_id(&self, service_id: &str) -> Result<()> {
        match &self.ctx.service_id {
            Some(sid) => {
                if sid == service_id {
                    Ok(())
                } else {
                    Err(Error::RemoteServiceError(format!(
                        "Invalid destination service address: {}",
                        service_id
                    )))
                }
            }
            None => Ok(()),
        }
    }
}

impl<R: Runtime> Actor for ExeUnit<R> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let address = ctx.address();
        if let Err(e) = self.start_runtime() {
            log::error!("Failed to start runtime: {:?}", e);
            return address.do_send(Shutdown::default());
        }
        if let Some(service_id) = &self.ctx.service_id {
            actix_rpc_bind!(
                service_id,
                ctx.address(),
                [
                    activity_model::Exec,
                    activity_model::GetActivityState,
                    activity_model::GetActivityUsage,
                    activity_model::GetRunningCommand,
                    activity_model::GetExecBatchResults
                ]
            );
        }

        IntervalFunc::new(*DEFAULT_REPORT_INTERVAL, Self::report_usage)
            .finish()
            .spawn(ctx);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        match &self.state.state {
            StateExt::State(s) => match s {
                State::Terminated => Running::Stop,
                _ => Running::Continue,
            },
            _ => Running::Continue,
        }
    }
}

pub(crate) async fn report<M: RpcMessage + Unpin + 'static>(url: &String, msg: M) {
    let result = ya_service_bus::typed::service(url)
        .send(msg)
        .map_err(Error::from)
        .await;

    if let Err(e) = result {
        log::warn!("Error reporting to {}: {:?}", url, e);
    }
}
