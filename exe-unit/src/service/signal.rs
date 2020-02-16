use crate::commands::{Shutdown, Signal};
use crate::service::Service;
use actix::dev::ToEnvelope;
use actix::prelude::*;
use std::fmt::Debug;

pub struct SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Signal>,
{
    signals: Vec<signal_hook::SigId>,
    parent: Option<Addr<A>>,
}

impl<A> SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Signal>,
{
    pub fn new() -> Self {
        SignalMonitor {
            signals: Vec::new(),
            parent: None,
        }
    }
}

macro_rules! register_signal {
    ($handler:expr, $sig:expr) => {{
        let handler_ = $handler.clone().unwrap();
        let f = move || {
            handler_.do_send(Signal($sig));
        };
        unsafe { signal_hook::register($sig, f).unwrap() }
    }};
}

impl<A> Service for SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Signal>,
    <A as Actor>::Context: ToEnvelope<A, Signal>,
{
    const ID: &'static str = "SignalMonitor";
    type Parent = A;

    fn bind(&mut self, parent: Addr<A>) {
        self.parent = Some(parent);
    }
}

impl<A> Actor for SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Signal>,
    <A as Actor>::Context: ToEnvelope<A, Signal>,
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
            .push(register_signal!(self.parent, signal_hook::SIGHUP));
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
    A: Actor<Context = Context<A>> + Handler<Signal>,
    <A as Actor>::Context: ToEnvelope<A, Signal>,
{
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}
