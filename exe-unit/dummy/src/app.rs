use anyhow::{anyhow, Result};
use api::core::{Cmd, Context};
use futures::{
    future::BoxFuture,
    lock::Mutex,
};
use std::{
    sync::Arc,
    time::Duration,
};
use tokio::time::delay_for;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum State {
    Ready,
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
            state: Arc::new(Mutex::new(State::Ready)),
        }
    }

    fn is_running(&self) -> bool {
        *self.state.lock().unwrap() == State::Running
    }

    fn is_ready(&self) -> bool {
        *self.state.try_lock().unwrap() == State::Ready
    }

    fn is_finished(&self) -> bool {
        *self.state.try_lock().unwrap() == State::Finished
    }

    fn state(&self) -> State {
        *self.state.try_lock().unwrap()
    }
}

impl Context for DummyExeUnit {}

#[derive(Debug)]
pub struct Start {
    pub params: Vec<String>,
}

impl Cmd<DummyExeUnit> for Start {
    type Response = Result<State>;
    type Result = BoxFuture<'static, Self::Response>;

    fn action(self, ctx: DummyExeUnit) -> Self::Result {
        if ctx.is_running() {
            return Box::pin(async { Err(anyhow!("container is already started")) });
        }

        Box::pin(async move {
            delay_for(Duration::from_secs(5)).await;
            let state = State::Finished;
            *ctx.state.lock().await = state;
            Ok(state)
        })
    }
}

#[derive(Debug)]
pub struct Status;

impl Cmd<DummyExeUnit> for Status {
    type Response = State;
    type Result = BoxFuture<'static, Self::Response>;

    fn action(self, ctx: DummyExeUnit) -> Self::Result {
        Box::pin(async move { ctx.state() })
    }
}
