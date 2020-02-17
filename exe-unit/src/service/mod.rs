pub mod metrics;
pub mod signal;

use crate::commands::Shutdown;
use actix::prelude::*;

pub trait ServiceControl {
    fn stop(&mut self);
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
    fn stop(&mut self) {
        self.addr.do_send(Shutdown::default())
    }
}
