use crate::ExeUnitError;
use anyhow::Result;
use futures::{
    channel::oneshot,
    future::{BoxFuture, Future},
};
use std::fmt::Debug;

pub trait HandleCmd<Cmd, Output>
where
    Cmd: Sync + Send,
{
    type Result: Future<Output = Output>;

    fn handle(&mut self, cmd: Cmd) -> Self::Result;
}

pub trait ExeUnit: Send + Sync + Clone {
    type State: Clone + Send + Sync + Debug;

    fn is_ready(&self) -> bool;
    fn is_running(&self) -> bool;
    fn is_finished(&self) -> bool;

    fn state(&self) -> Self::State;
    fn start(&mut self, params: Vec<String>) -> BoxFuture<Result<()>>;
}

#[derive(Debug)]
pub struct StartCmd {
    pub params: Vec<String>,
}

impl<T> HandleCmd<StartCmd, Result<T::State>> for T
where
    T: ExeUnit + 'static,
{
    type Result = BoxFuture<'static, Result<T::State>>;

    fn handle(&mut self, cmd: StartCmd) -> Self::Result {
        if self.is_running() {
            return Box::pin(async { Err(ExeUnitError::OpInProgress("start".to_string()).into()) });
        }

        let (tx, rx) = oneshot::channel();
        let mut ctx = self.clone();
        tokio::spawn(async move {
            let res = ctx.start(cmd.params).await;
            tx.send(res).expect("sending notification should not fail");
        });

        let ctx = self.clone();
        Box::pin(async move {
            rx.await??;
            Ok(ctx.state())
        })
    }
}

#[derive(Debug)]
pub struct StatusCmd;

impl<T> HandleCmd<StatusCmd, T::State> for T
where
    T: ExeUnit + 'static,
{
    type Result = BoxFuture<'static, T::State>;

    fn handle(&mut self, _: StatusCmd) -> Self::Result {
        let state = self.state();
        Box::pin(async move { state })
    }
}
