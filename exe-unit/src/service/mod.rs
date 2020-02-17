pub mod default;
pub mod metrics;
pub mod reporter;
pub mod signal;

use crate::commands::Shutdown;
use actix::prelude::*;

pub trait Service: Actor<Context = Context<Self>> + Handler<Shutdown> {}

pub trait ServiceControl {
    fn stop(&mut self);
}

pub(crate) struct ServiceAddr<S: Service> {
    addr: Addr<S>,
}

impl<S: Service> ServiceAddr<S> {
    pub fn new(service: Addr<S>) -> Self {
        ServiceAddr { addr: service }
    }
}

impl<S: Service> ServiceControl for ServiceAddr<S> {
    fn stop(&mut self) {
        self.addr.do_send(Shutdown::default())
    }
}
