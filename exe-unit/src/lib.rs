pub mod cli;
pub mod commands;
pub mod error;
mod handlers;
pub mod runtime;
pub mod service;

use crate::commands::*;
use crate::runtime::*;
use crate::service::Service;

use actix::prelude::*;
use futures::SinkExt;
use serde_json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use ya_core_model::activity as activity_model;
use ya_model::activity::State;
use ya_service_bus::actix_rpc;
use ya_service_bus::timeout::IntoTimeoutFuture;

pub type Result<T> = std::result::Result<T, error::Error>;

#[derive(Clone, Debug)]
pub struct ExeUnitContext {
    service_id: Option<String>,
    config_path: Option<PathBuf>,
    input_dir: PathBuf,
    output_dir: PathBuf,
}

pub struct ExeUnitState {
    pub state: StateExt,
    batch_results: HashMap<String, Vec<Vec<u8>>>,
    pub running_command: Option<RuntimeCommand>,
}

impl ExeUnitState {
    pub fn get_results(&self, batch_id: &String) -> Vec<Vec<u8>> {
        match self.batch_results.get(batch_id) {
            Some(vec) => vec.clone(),
            None => Vec::new(),
        }
    }

    pub fn push_result(&mut self, batch_id: String, result: Vec<u8>) {
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
    services: Vec<Box<dyn Service<Self>>>,
}

macro_rules! actix_rpc_bind {
    ($sid:expr, $addr:expr, [$($ty:ty),*]) => {
        $(
            actix_rpc::bind::<$ty>($sid, $addr.clone().recipient());
        )*
    };
}

impl<R: Runtime> ExeUnit<R> {
    pub fn new(ctx: ExeUnitContext) -> Self {
        Self {
            ctx,
            state: ExeUnitState::default(),
            runtime: None,
            services: Vec::new(),
        }
    }

    pub fn service<S>(mut self, service: S) -> Self
    where
        S: Service<Self> + 'static,
    {
        self.services.push(Box::new(service));
        self
    }

    fn start_services(&mut self, actor: Addr<Self>) -> Result<()> {
        for svc in self.services.iter_mut() {
            svc.start(actor.clone())?;
        }
        Ok(())
    }

    fn start_runtime(&mut self) -> Result<()> {
        let config_path = self.ctx.config_path.clone();
        let input_dir = self.ctx.input_dir.clone();
        let output_dir = self.ctx.output_dir.clone();
        let runtime = RuntimeThread::spawn(move || {
            R::new(config_path.clone(), input_dir.clone(), output_dir.clone())
        })?;
        self.runtime = Some(runtime);
        Ok(())
    }
}

impl<R: Runtime> Actor for ExeUnit<R> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        if let Err(e) = self.start_services(ctx.address()) {
            log::error!("Failed to start services: {:?}", e);
            self.state.state = StateExt::State(State::Terminated);
            return Arbiter::current().stop();
        }
        if let Err(e) = self.start_runtime() {
            log::error!("Failed to start runtime: {:?}", e);
            self.state.state = StateExt::State(State::Terminated);
            return Arbiter::current().stop();
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
    }

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        match &self.state.state {
            StateExt::State(s) => match s {
                State::Terminated => Running::Stop,
                _ => Running::Continue,
            },
            _ => Running::Continue,
        }
    }
}
