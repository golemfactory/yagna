use crate::commands::Signal;
use crate::service::Service;
use crate::Result;
use actix::dev::ToEnvelope;
use actix::{Actor, Addr, Handler};
use std::fmt::Debug;
use std::marker::PhantomData;

#[derive(Debug)]
pub struct SignalMonitor<A: Actor>
where
    A: Actor + Handler<Signal>,
{
    signals: Vec<signal_hook::SigId>,
    phantom: PhantomData<A>,
}

macro_rules! register_signal {
    ($handler:ident, $sig:expr) => {{
        let handler_ = $handler.clone();
        let f = move || {
            handler_.do_send(Signal($sig));
        };
        unsafe { signal_hook::register($sig, f)? }
    }};
}

impl<A> Service<A> for SignalMonitor<A>
where
    A: Actor + Handler<Signal> + Debug,
    <A as Actor>::Context: ToEnvelope<A, Signal>,
{
    fn start(&mut self, actor: Addr<A>) -> Result<()> {
        self.signals
            .push(register_signal!(actor, signal_hook::SIGABRT));
        self.signals
            .push(register_signal!(actor, signal_hook::SIGINT));
        self.signals
            .push(register_signal!(actor, signal_hook::SIGTERM));
        #[cfg(not(windows))]
        self.signals
            .push(register_signal!(actor, signal_hook::SIGHUP));
        #[cfg(not(windows))]
        self.signals
            .push(register_signal!(actor, signal_hook::SIGQUIT));

        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        std::mem::replace(&mut self.signals, Vec::new())
            .into_iter()
            .for_each(|s| {
                signal_hook::unregister(s);
            });

        Ok(())
    }
}
