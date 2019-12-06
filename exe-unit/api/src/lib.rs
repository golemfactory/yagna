use futures::{
    channel::oneshot,
    future::{BoxFuture, Future},
    stream::{self, BoxStream, Stream, StreamExt},
};
use serde::de::DeserializeOwned;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("oneshot channel Sender prematurely dropped")]
    OneshotCanceled(#[from] oneshot::Canceled),
    #[error("deserializing command failed with {0:?}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("expected a JSON array object; got {0}")]
    WrongJson(serde_json::Value),
}

pub trait Context: Clone + Send + Sync {}

pub trait Command<Ctx>
where
    Self: Send + Sync,
    Ctx: Context + 'static,
{
    type Response: Send + 'static;

    fn action(self, ctx: Ctx) -> BoxFuture<'static, Self::Response>;
}

pub trait Handle<Cmd>
where
    Self: Context + 'static,
    Cmd: Command<Self> + 'static,
{
    type Result: Future<Output = Result<Cmd::Response, ApiError>> + Send;

    fn handle(&mut self, cmd: Cmd) -> Self::Result;
}

impl<Ctx, Cmd> Handle<Cmd> for Ctx
where
    Ctx: Context + 'static,
    Cmd: Command<Ctx> + 'static,
{
    type Result = BoxFuture<'static, Result<Cmd::Response, ApiError>>;

    fn handle(&mut self, cmd: Cmd) -> Self::Result {
        let (tx, rx) = oneshot::channel();
        let ctx = self.clone();
        tokio::spawn(async move {
            let res = cmd.action(ctx).await;
            if let Err(_) = tx.send(res) {
                log::error!("send should succeed")
            }
        });
        Box::pin(async move { rx.await.map_err(Into::into) })
    }
}

pub trait Exec<Cmd>
where
    Self: Context + 'static,
    Cmd: Command<Self> + DeserializeOwned + 'static,
{
    type Result: Stream<Item = Result<Cmd::Response, ApiError>>;

    fn exec(&mut self, cmd_json: String) -> Self::Result;
}

impl<Ctx, Cmd> Exec<Cmd> for Ctx
where
    Ctx: Context + Handle<Cmd> + 'static,
    Cmd: Command<Ctx> + DeserializeOwned + 'static,
{
    type Result = BoxStream<'static, Result<Cmd::Response, ApiError>>;

    fn exec(&mut self, cmd_json: String) -> Self::Result {
        // we expect input to be a JSON Array even if it consists of one element
        match serde_json::from_str(&cmd_json) {
            Err(e) => Box::pin(stream::once(async { Err(e.into()) })),
            // OK, we got a JSON array, now interpret Values as strongly typed Cmd
            // in case we cannot interpret any command, throw an error but do not
            // interrupt the flow
            Ok(serde_json::Value::Array(cmds)) => {
                let cmds: Vec<_> = cmds.into_iter().map(|cmd| (self.clone(), cmd)).collect();
                Box::pin(stream::iter(cmds).then(|(mut ctx, cmd)| {
                    async move {
                        let cmd = serde_json::from_value(cmd)?;
                        ctx.handle(cmd).await.map_err(Into::into)
                    }
                }))
            }
            Ok(other) => Box::pin(stream::once(async { Err(ApiError::WrongJson(other)) })),
        }
    }
}
