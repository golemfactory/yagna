pub mod metrics;
pub mod signal;

use crate::message::{Shutdown, ShutdownReason};
use actix::prelude::*;
use futures::future::LocalBoxFuture;
use futures::FutureExt;

pub trait ServiceControl {
    fn stop(&mut self) -> LocalBoxFuture<()>;
}

pub(crate) struct ServiceAddr<Svc>
where
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>,
{
    addr: Addr<Svc>,
}

impl<Svc> ServiceAddr<Svc>
where
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>,
{
    pub fn new(service: Addr<Svc>) -> Self {
        ServiceAddr { addr: service }
    }
}

impl<Svc> ServiceControl for ServiceAddr<Svc>
where
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>,
{
    fn stop(&mut self) -> LocalBoxFuture<()> {
        let addr = self.addr.clone();
        async move {
            let _ = addr.send(Shutdown(ShutdownReason::Finished)).await;
        }
        .boxed_local()
    }
}
