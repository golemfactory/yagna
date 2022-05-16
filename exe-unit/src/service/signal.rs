use crate::message::{Shutdown, ShutdownReason};
use actix::dev::ToEnvelope;
use actix::prelude::*;
use signal_hook::{
    consts::*,
    low_level::{register, unregister},
    SigId,
};

pub struct SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Shutdown>,
{
    signals: Vec<SigId>,
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

fn register_signal<A>(addr: &Addr<A>, signal: i32) -> SigId
where
    A: Actor<Context = Context<A>> + Handler<Shutdown>,
    <A as Actor>::Context: ToEnvelope<A, Shutdown>,
{
    let handler_ = addr.clone();
    let f = move || {
        log::info!("Caught signal: {}", signal);
        handler_.do_send(Shutdown(ShutdownReason::Interrupted(signal)));
    };

    unsafe { register(signal, f) }.unwrap()
}

impl<A> Actor for SignalMonitor<A>
where
    A: Actor<Context = Context<A>> + Handler<Shutdown>,
    <A as Actor>::Context: ToEnvelope<A, Shutdown>,
{
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        self.signals.push(register_signal(&self.parent, SIGABRT));
        self.signals.push(register_signal(&self.parent, SIGINT));
        self.signals.push(register_signal(&self.parent, SIGTERM));
        #[cfg(not(windows))]
        self.signals.push(register_signal(&self.parent, SIGQUIT));

        log::debug!("Signal monitoring service started");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        std::mem::replace(&mut self.signals, Vec::new())
            .into_iter()
            .for_each(|s| {
                unregister(s);
            });

        log::debug!("Signal monitoring service stopped");
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
