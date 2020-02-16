use crate::commands::Shutdown;
use crate::error::Error;
use crate::service::Service;
use actix::prelude::*;
use futures::TryFutureExt;
use std::marker::PhantomData;
use std::time::Duration;
use ya_service_bus::{RpcEndpoint, RpcMessage};

struct Reporter<P, F, M>
where
    P: Actor<Context = Context<P>>,
    F: Fn() -> M + 'static,
    M: RpcMessage + Unpin,
{
    remote_addr: String,
    interval: Duration,
    data_source: Box<F>,
    phantom: PhantomData<P>,
}

impl<P, F, M> Reporter<P, F, M>
where
    P: Actor<Context = Context<P>>,
    F: Fn() -> M + 'static,
    M: RpcMessage + Unpin,
{
    pub fn new(remote_addr: impl ToString, interval: Duration, data_source: F) -> Self {
        Self {
            remote_addr: remote_addr.to_string(),
            interval,
            data_source: Box::new(data_source),
            phantom: PhantomData,
        }
    }

    fn report(&mut self, context: &mut Context<Self>) {
        let msg = (self.data_source)();
        let addr = self.remote_addr.clone();

        let fut = async move {
            let result = ya_service_bus::typed::service(&addr)
                .send(msg)
                .map_err(Error::from)
                .await
                .map_err(|e| Error::RemoteServiceError(format!("{:?}", e)));

            if let Err(e) = result {
                log::warn!("Error reporting to {}: {:?}", addr, e);
            }
        };

        context.spawn(fut.into_actor(self));
    }
}

impl<P, F, M> Actor for Reporter<P, F, M>
where
    P: Actor<Context = Context<P>>,
    F: Fn() -> M + 'static,
    M: RpcMessage + Unpin,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        IntervalFunc::new(self.interval, Self::report)
            .finish()
            .spawn(ctx);
    }
}

impl<P, F, M> Handler<Shutdown> for Reporter<P, F, M>
where
    P: Actor<Context = Context<P>>,
    F: Fn() -> M + 'static,
    M: RpcMessage + Unpin,
{
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}

impl<P, F, M> Service for Reporter<P, F, M>
where
    P: Actor<Context = Context<P>>,
    F: Fn() -> M + 'static,
    M: RpcMessage + Unpin,
{
    type Parent = P;
}
