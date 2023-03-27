use futures::channel::mpsc;
use futures::{Future, SinkExt, Stream};
use futures_util::task::{Context, Poll};
use std::pin::Pin;

use signal_hook::{
    consts::*,
    low_level::{register, unregister},
    SigId,
};

pub(crate) type Signal = (i32, &'static str);

pub struct SignalMonitor {
    rx: mpsc::Receiver<Signal>,
    hooks: Vec<SigId>,
}

impl SignalMonitor {
    pub fn new(signals: Vec<i32>) -> Self {
        let (tx, rx) = mpsc::channel(1);
        let hooks = signals
            .into_iter()
            .map(|s| register_signal(tx.clone(), s))
            .collect();

        SignalMonitor { rx, hooks }
    }
}

impl Default for SignalMonitor {
    fn default() -> Self {
        #[allow(unused)]
        let mut signals = vec![SIGABRT, SIGINT, SIGTERM];

        #[cfg(not(windows))]
        signals.push(SIGQUIT);

        Self::new(signals)
    }
}

impl Future for SignalMonitor {
    type Output = Signal;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.rx).poll_next(cx) {
            Poll::Ready(Some(s)) => Poll::Ready(s),
            Poll::Ready(None) | Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for SignalMonitor {
    fn drop(&mut self) {
        std::mem::take(&mut self.hooks).into_iter().for_each(|s| {
            unregister(s);
        });
    }
}

fn register_signal(tx: mpsc::Sender<Signal>, signal: i32) -> SigId {
    log::trace!("Registering signal {} ({})", signal_to_str(signal), signal);

    let action = move || {
        let mut tx = tx.clone();
        tokio::spawn(async move {
            let signal_pair = (signal, signal_to_str(signal));
            if let Err(e) = tx.send(signal_pair).await {
                log::error!("Unable to notify about signal {:?}: {}", signal_pair, e);
            }
        });
    };

    unsafe { register(signal, action) }.unwrap()
}

fn signal_to_str(signal: i32) -> &'static str {
    match signal {
        #[cfg(not(windows))]
        SIGHUP => "SIGHUP",
        #[cfg(not(windows))]
        SIGQUIT => "SIGQUIT",
        #[cfg(not(windows))]
        SIGKILL => "SIGKILL",
        #[cfg(not(windows))]
        SIGPIPE => "SIGPIPE",
        #[cfg(not(windows))]
        SIGALRM => "SIGALRM",
        SIGINT => "SIGINT",
        SIGILL => "SIGILL",
        SIGABRT => "SIGABRT",
        SIGFPE => "SIGFPE",
        SIGSEGV => "SIGSEGV",
        SIGTERM => "SIGTERM",
        _ => "SIG?",
    }
}
