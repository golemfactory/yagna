use crate::{
    command::{one::Command, Dispatcher},
    Error, Result,
};
use actix::{
    dev::{MessageResponse, ResponseChannel, ToEnvelope},
    prelude::*,
};
use futures::{
    future,
    stream::{self, Stream},
};
use std::marker::PhantomData;

type BoxStream<I, E> = Box<dyn Stream<Item = I, Error = E> + Send>;

pub struct StreamResponse<I, E>(BoxStream<I, E>)
where
    I: 'static,
    E: 'static;

impl<I, E> StreamResponse<I, E>
where
    I: 'static,
    E: 'static,
{
    pub fn new(inner: impl Stream<Item = I, Error = E> + Send + 'static) -> Self {
        Self(Box::new(inner))
    }

    pub fn into_inner(self) -> impl Stream<Item = I, Error = E> + Send {
        self.0
    }
}

impl<A, M, I, E> MessageResponse<A, M> for StreamResponse<I, E>
where
    A: Actor,
    M: Message<Result = Self>,
    I: 'static,
    E: 'static,
{
    fn handle<R: ResponseChannel<M>>(self, _: &mut A::Context, tx: Option<R>) {
        if let Some(tx) = tx {
            tx.send(self)
        }
    }
}

pub struct Commands<M, R, Ctx>
where
    M: Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M> + 'static,
{
    inner: Vec<M>,
    phantom: PhantomData<Ctx>,
}

impl<M, R, Ctx> Commands<M, R, Ctx>
where
    M: Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M> + 'static,
{
    pub fn new(inner: Vec<M>) -> Self {
        Self {
            inner,
            phantom: PhantomData,
        }
    }
}

impl<M, R, Ctx> Message for Commands<M, R, Ctx>
where
    M: Message<Result = R>,
    R: 'static,
    Ctx: Actor + Handler<M> + 'static,
{
    type Result = StreamResponse<Result<R>, ()>;
}

impl<M, R, Ctx> Handler<Commands<M, R, Ctx>> for Dispatcher<Ctx>
where
    M: Message<Result = R> + Send + 'static,
    R: Send + 'static,
    Ctx: Actor + Handler<M> + Send + 'static,
    <Ctx as Actor>::Context: AsyncContext<Ctx> + ToEnvelope<Ctx, M>,
{
    type Result = StreamResponse<Result<R>, ()>;

    fn handle(&mut self, msg: Commands<M, R, Ctx>, ctx: &mut Self::Context) -> Self::Result {
        let dispatcher = ctx.address();
        let cmds: Vec<_> = msg
            .inner
            .into_iter()
            .map(|cmd| (dispatcher.clone(), cmd))
            .collect();
        StreamResponse::new(stream::iter_ok(cmds).and_then(|(dispatcher, cmd)| {
            dispatcher.send(Command::new(cmd)).then(|res| match res {
                Err(e) => future::ok(Err(Error::from(e))),
                Ok(res) => future::ok(res),
            })
        }))
    }
}
