use futures::channel::mpsc;
use futures::{Future, SinkExt, Stream};
use futures_util::task::{Context, Poll};
use std::pin::Pin;

pub(crate) type Signal = (i32, &'static str);

pub(crate) struct SignalMonitor {
    rx: mpsc::Receiver<Signal>,
    hooks: Vec<signal_hook::SigId>,
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
        let mut signals = vec![
            signal_hook::SIGABRT,
            signal_hook::SIGINT,
            signal_hook::SIGTERM,
        ];

        #[cfg(not(windows))]
        signals.push(signal_hook::SIGQUIT);

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
        std::mem::replace(&mut self.hooks, Vec::new())
            .into_iter()
            .for_each(|s| {
                signal_hook::unregister(s);
            });
    }
}

fn register_signal(tx: mpsc::Sender<Signal>, signal: i32) -> signal_hook::SigId {
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

    unsafe { signal_hook::register(signal, action) }.unwrap()
}

fn signal_to_str(signal: i32) -> &'static str {
    match signal {
        #[cfg(not(windows))]
        signal_hook::SIGHUP => "SIGHUP",
        #[cfg(not(windows))]
        signal_hook::SIGQUIT => "SIGQUIT",
        #[cfg(not(windows))]
        signal_hook::SIGKILL => "SIGKILL",
        #[cfg(not(windows))]
        signal_hook::SIGPIPE => "SIGPIPE",
        #[cfg(not(windows))]
        signal_hook::SIGALRM => "SIGALRM",
        signal_hook::SIGINT => "SIGINT",
        signal_hook::SIGILL => "SIGILL",
        signal_hook::SIGABRT => "SIGABRT",
        signal_hook::SIGFPE => "SIGFPE",
        signal_hook::SIGSEGV => "SIGSEGV",
        signal_hook::SIGTERM => "SIGTERM",
        _ => "SIG?",
    }
}
