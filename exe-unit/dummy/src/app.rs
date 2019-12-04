use anyhow::Result;
use api::core::ExeUnit;
use futures::future::BoxFuture;
use std::{
    sync::{Arc, Mutex},
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

impl ExeUnit for DummyExeUnit {
    type State = State;

    fn is_running(&self) -> bool {
        *self.state.lock().unwrap() == State::Running
    }

    fn is_ready(&self) -> bool {
        *self.state.lock().unwrap() == State::Ready
    }

    fn is_finished(&self) -> bool {
        *self.state.lock().unwrap() == State::Finished
    }

    fn state(&self) -> Self::State {
        *self.state.lock().unwrap()
    }

    fn start(&mut self, _params: Vec<String>) -> BoxFuture<Result<()>> {
        let state = self.state.clone();
        Box::pin(async move {
            delay_for(Duration::from_secs(5)).await;
            *state.lock().unwrap() = State::Finished;
            Ok(())
        })
    }
}

impl DummyExeUnit {
    pub fn spawn() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::Ready)),
        }
    }
}
