use crate::commands::{Shutdown, ShutdownReason};
use crate::service::Service;
use actix::dev::ToEnvelope;
use actix::prelude::*;
use serde::{Deserialize, Serialize};

pub struct SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Shutdown>,
{
    signals: Vec<signal_hook::SigId>,
    parent: Addr<A>,
}

impl<A> SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Shutdown>,
{
    pub fn new(parent: Addr<A>) -> Self {
        SignalMonitor {
            signals: Vec::new(),
            parent,
        }
    }
}

macro_rules! register_signal {
    ($handler:expr, $sig:expr) => {{
        let handler_ = $handler.clone();
        let f = move || {
            handler_.do_send(Shutdown(ShutdownReason::Interrupted($sig as i32)));
        };
        unsafe { signal_hook::register($sig, f).unwrap() }
    }};
}

impl<A> Service for SignalMonitor<A> where A: Actor<Context = Context<A>> + Handler<Shutdown> {}

impl<A> Actor for SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Shutdown>,
    <A as Actor>::Context: ToEnvelope<A, Shutdown>,
{
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        self.signals
            .push(register_signal!(self.parent, signal_hook::SIGABRT));
        self.signals
            .push(register_signal!(self.parent, signal_hook::SIGINT));
        self.signals
            .push(register_signal!(self.parent, signal_hook::SIGTERM));
        #[cfg(not(windows))]
        self.signals
            .push(register_signal!(self.parent, signal_hook::SIGQUIT));
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        std::mem::replace(&mut self.signals, Vec::new())
            .into_iter()
            .for_each(|s| {
                signal_hook::unregister(s);
            });
    }
}

impl<A> Handler<Shutdown> for SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Shutdown>,
    <A as Actor>::Context: ToEnvelope<A, Shutdown>,
{
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}
