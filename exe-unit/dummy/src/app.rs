use anyhow::{anyhow, Result};
use api::{Cmd, Context};
use futures::future::BoxFuture;
use serde::Deserialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::delay_for;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum State {
    Null,
    Deployed,
    Running,
    Finished,
}

#[derive(Clone)]
pub struct DummyExeUnit {
    state: Arc<Mutex<State>>,
}

impl DummyExeUnit {
    pub fn spawn() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::Null)),
        }
    }

    fn is_null(&self) -> bool {
        *self.state.lock().unwrap() == State::Null
    }

    fn is_running(&self) -> bool {
        *self.state.lock().unwrap() == State::Running
    }

    fn state(&self) -> State {
        *self.state.lock().unwrap()
    }
}

impl Context for DummyExeUnit {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DummyCmd {
    Deploy { params: Vec<String> },
    Transfer { from: String, to: String },
    Start { params: Vec<String> },
    Status,
}

impl DummyCmd {
    fn deploy(ctx: DummyExeUnit, _params: Vec<String>) -> BoxFuture<'static, Result<State>> {
        if !ctx.is_null() {
            return Box::pin(async { Err(anyhow!("container is already deployed")) });
        }

        Box::pin(async move {
            let state = State::Deployed;
            *ctx.state.lock().unwrap() = state;
            Ok(state)
        })
    }

    fn transfer(
        ctx: DummyExeUnit,
        _from: String,
        _to: String,
    ) -> BoxFuture<'static, Result<State>> {
        if ctx.is_running() {
            return Box::pin(async { Err(anyhow!("container is currently running")) });
        }

        Box::pin(async move { Ok(*ctx.state.lock().unwrap()) })
    }

    fn start(ctx: DummyExeUnit, _params: Vec<String>) -> BoxFuture<'static, Result<State>> {
        if ctx.is_running() {
            return Box::pin(async { Err(anyhow!("container is already started")) });
        }

        Box::pin(async move {
            delay_for(Duration::from_secs(5)).await;
            let state = State::Finished;
            *ctx.state.lock().unwrap() = state;
            Ok(state)
        })
    }

    fn status(ctx: DummyExeUnit) -> BoxFuture<'static, Result<State>> {
        Box::pin(async move { Ok(ctx.state()) })
    }
}

impl Cmd<DummyExeUnit> for DummyCmd {
    type Response = Result<State>;

    fn action(self, ctx: DummyExeUnit) -> BoxFuture<'static, Self::Response> {
        match self {
            Self::Deploy { params } => Self::deploy(ctx, params),
            Self::Transfer { from, to } => Self::transfer(ctx, from, to),
            Self::Start { params } => Self::start(ctx, params),
            Self::Status => Self::status(ctx),
        }
    }
}
