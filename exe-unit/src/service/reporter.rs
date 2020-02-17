use crate::commands::Shutdown;
use crate::error::Error;
use crate::service::Service;
use crate::Result;
use actix::prelude::*;
use futures::TryFutureExt;
use std::pin::Pin;
use std::time::Duration;
use ya_service_bus::{RpcEndpoint, RpcMessage};

//#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
//#[rtype(result = "()")]
//pub struct Report<M: RpcMessage + Unpin>(pub M);

pub struct Reporter<F, M>
where
    F: Fn() -> Pin<Box<dyn Future<Output = Result<M>>>> + 'static,
    M: RpcMessage + Unpin,
{
    remote_addr: String,
    interval: Duration,
    data_source: Box<F>,
}

impl<F, M> Reporter<F, M>
where
    F: Fn() -> Pin<Box<dyn Future<Output = Result<M>>>> + 'static,
    M: RpcMessage + Unpin,
{
    pub fn new(remote_addr: impl ToString, interval: Duration, data_source: F) -> Self {
        Self {
            remote_addr: remote_addr.to_string(),
            interval,
            data_source: Box::new(data_source),
        }
    }

    fn report(&mut self, context: &mut Context<Self>) {
        let addr = self.remote_addr.clone();
        let data = (self.data_source)();
        let fut = async move {
            match data.await {
                Ok(msg) => {
                    let result = ya_service_bus::typed::service(&addr)
                        .send(msg)
                        .map_err(Error::from)
                        .await
                        .map_err(|e| Error::RemoteServiceError(format!("{:?}", e)));

                    if let Err(e) = result {
                        log::warn!("Error reporting to {}: {:?}", addr, e);
                    }
                }
                Err(e) => {
                    log::error!("Reporting data source error: {:?}", e);
                }
            };
        };

        context.spawn(fut.into_actor(self));
    }
}

impl<F, M> Actor for Reporter<F, M>
where
    F: Fn() -> Pin<Box<dyn Future<Output = Result<M>>>> + 'static,
    M: RpcMessage + Unpin,
{
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        IntervalFunc::new(self.interval, Self::report)
            .finish()
            .spawn(ctx);
    }
}

//impl<F, M> Handler<Report<M>> for Reporter<F, M>
//where
//    F: Fn() -> Pin<Box<dyn Future<Output = Result<M>>>> + 'static,
//    M: RpcMessage + Unpin,
//{
//    type Result = <Report<M> as Message>::Result;
//
//    fn handle(&mut self, _: Report<M>, ctx: &mut Self::Context) -> Self::Result {
//        ctx.stop();
//        Ok(())
//    }
//}

impl<F, M> Handler<Shutdown> for Reporter<F, M>
where
    F: Fn() -> Pin<Box<dyn Future<Output = Result<M>>>> + 'static,
    M: RpcMessage + Unpin,
{
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}

impl<F, M> Service for Reporter<F, M>
where
    F: Fn() -> Pin<Box<dyn Future<Output = Result<M>>>> + 'static,
    M: RpcMessage + Unpin,
{
}
