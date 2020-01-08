use crate::{command::Dispatcher, Error, Result};
use actix::{dev::ToEnvelope, prelude::*};
use futures::{channel::oneshot, future::{self, Future}, prelude::*, FutureExt};
use std::marker::PhantomData;

pub struct Command<M, R, Ctx>
where
    M: Message<Result = R> + Unpin,
    R: 'static,
    Ctx: Actor + Handler<M> + 'static,
{
    inner: M,
    phantom: PhantomData<Ctx>,
}

impl<M, R, Ctx> Command<M, R, Ctx>
where
    M: Message<Result = R> + Unpin,
    R: 'static,
    Ctx: Actor + Handler<M> + 'static,
{
    pub fn new(inner: M) -> Self {
        Self {
            inner,
            phantom: PhantomData,
        }
    }
}

impl<M, R, Ctx> Message for Command<M, R, Ctx>
where
    M: Message<Result = R> + Unpin,
    R: 'static,
    Ctx: Actor + Handler<M> + 'static,
{
    type Result = Result<M::Result>;
}

impl<M, R, Ctx> Handler<Command<M, R, Ctx>> for Dispatcher<Ctx>
where
    M: Message<Result = R> + Send + Unpin + 'static,
    R: Send + 'static,
    Ctx: Actor + Handler<M> + Send + 'static,
    <Ctx as Actor>::Context: AsyncContext<Ctx> + ToEnvelope<Ctx, M>,
{
    type Result = ActorResponse<Self, M::Result, Error>;

    fn handle(&mut self, msg: Command<M, R, Ctx>, _: &mut Self::Context) -> Self::Result {
        let (mut tx, rx) = oneshot::channel();
        let recipient = self.worker.clone();
        Arbiter::new().send(recipient.send(msg.inner).then(move |res| {
            if let Err(_) = tx.send(res.map_err(From::from)) {
                log::error!("send should succeed");
            }
            future::ready(())
        }));
        ActorResponse::r#async(rx.then(|v| async{v?}).into_actor(self))
    }
}
