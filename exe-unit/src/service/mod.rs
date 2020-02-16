pub mod reporter;
pub mod signal;

use crate::commands::Shutdown;
use actix::prelude::*;

pub trait Service: Actor<Context = Context<Self>> + Handler<Shutdown> {
    type Parent: Actor<Context = Context<Self::Parent>>;

    /// Set the parent actor address. Always called before starting the service.
    fn bind(&mut self, parent: Addr<Self::Parent>) {}
}

pub trait ServiceControl {
    type Parent: Actor;

    fn start(&mut self, parent: Addr<Self::Parent>);
    fn stop(&mut self);
}

pub enum ServiceState<S: Service> {
    Initial(S),
    Running(Addr<S>),
    Stopped,
    Invalid,
}

impl<S: Service> ServiceState<S> {
    pub fn new(service: S) -> Self {
        ServiceState::Initial(service)
    }
}

impl<A, S> ServiceControl for ServiceState<S>
where
    A: Actor<Context = Context<A>>,
    S: Service<Parent = A>,
{
    type Parent = A;

    fn start(&mut self, parent: Addr<A>) {
        *self = match std::mem::replace(self, ServiceState::<S>::Invalid) {
            ServiceState::Initial(mut svc) => {
                svc.bind(parent);
                ServiceState::Running(svc.start())
            }
            state => state,
        };
    }

    fn stop(&mut self) {
        *self = match std::mem::replace(self, ServiceState::<S>::Invalid) {
            ServiceState::Running(addr) => {
                addr.do_send(Shutdown::new());
                ServiceState::<S>::Stopped
            }
            state => state,
        };
    }
}
